#!/usr/bin/env python3
"""Validate a local trial app through macOS Launch Services in isolated data directories.

The optional credential phase needs the existing opt-in loopback SSH fixture.
Only its isolated encrypted test vault is used and retained. Existing app windows and
user data are never selected or restarted by this runner.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import time
import uuid

from qa_accept_macos import Acceptance
from qa_control import process_running
from qa_credentials_macos import CredentialAcceptance
from package_release import digest


def binary_metadata(binary):
    """Inspect the tested Mach-O without relying on a local app packager."""
    archs = subprocess.check_output(["/usr/bin/lipo", "-archs", str(binary)], text=True).strip().split()
    libraries = []
    output = subprocess.check_output(["/usr/bin/otool", "-L", str(binary)], text=True)
    for line in output.splitlines()[1:]:
        library = line.strip().split(" (compatibility version", 1)[0]
        if not library.startswith(("/System/Library/", "/usr/lib/")):
            raise RuntimeError("External runtime library: " + library)
        libraries.append(library)
    build = subprocess.check_output(["/usr/bin/vtool", "-show-build", str(binary)], text=True)
    minimums = re.findall(r"\bminos\s+(\d+(?:\.\d+)+)", build)
    if not archs or not minimums:
        raise RuntimeError("Cannot determine architecture or deployment target")
    minimum = max(minimums, key=lambda value: tuple(int(part) for part in value.split(".")))
    return {"architectures": archs, "minimum_os_from_binary": minimum, "system_libraries": libraries}


def launch(bundle, directory):
    """Use Finder's Launch Services path with explicit test-only environment overrides."""
    directory.mkdir(parents=True, exist_ok=True)
    command = ["/usr/bin/open", "-n", "-g", "-W", "-a", str(bundle),
               "--stdout", str(directory / "stdout.log"), "--stderr", str(directory / "stderr.log")]
    for name, value in {
        "MANTASH_DATA_DIR": str(directory / "data"),
        "MANTASH_QA_CONTROL": str(directory / "command.json"),
        "MANTASH_QA_BACKGROUND": "1",
        # A Finder-launched app must not need Cargo, Node or development PATH entries.
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    }.items():
        command.extend(["--env", name + "=" + value])
    return subprocess.Popen(command, cwd=directory, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)


def wait_exit(process, pid):
    """Distinguish Launch Services' result from observation of native process termination."""
    _, error = process.communicate(timeout=20)
    if process.returncode:
        raise RuntimeError("Launch Services failed: " + error.strip())
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and process_running(pid):
        time.sleep(.05)
    if process_running(pid):
        raise RuntimeError("QA native process did not exit after its own quit action")
    return {"launch_services_exit_code": process.returncode, "native_process_exited": True}


def smoke(bundle, directory):
    """Validate a copied bundle away from source with a real Shell and restored preferences."""
    checks = Acceptance(directory, directory)
    launcher = launch(bundle, directory)
    pid = None
    try:
        state = checks.wait(lambda value: checks.pane(value)["state"] == "Connected", "Bundle launches a real local PTY through Launch Services")
        pid = state["pid"]
        actual = subprocess.check_output(["/bin/ps", "-p", str(pid), "-o", "comm="], text=True).strip()
        checks.check(actual == str(bundle / "Contents/MacOS/mantash"), "Running executable belongs to the copied app")
        checks.settled_terminal()
        checks.action("type", text="printf '%s\\n' 'bundle-中文-OK'\r")
        checks.wait(lambda value: "\nbundle-中文-OK\n" in checks.pane(value)["terminal"]["text"], "Bundle Shell produces real Unicode output")
        checks.action("settings")
        checks.check(checks.state()["modal"] == "settings", "Embedded fonts, icons and native settings load")
        checks.action("theme", night=True)
        checks.action("language", english=True)
        checks.action("dismiss")
        checks.action("local")
        checks.wait(lambda value: len(value["tabs"]) == 2 and checks.pane(value)["state"] == "Connected", "Copied app can start a second local Shell")
        checks.action("quit")
    finally:
        if pid and process_running(pid):
            last = json.loads((directory / "command.json").read_text())
            if last.get("action") != "quit":
                checks.action("dismiss")
                checks.action("quit")
        if pid:
            first_exit = wait_exit(launcher, pid)
    original_pid = pid
    launcher = launch(bundle, directory)
    try:
        state = checks.wait(lambda value: value["pid"] != original_pid and len(value["tabs"]) == 2,
                            "Launch Services restart restores saved tabs")
        pid = state["pid"]
        checks.check(state["preferences"]["theme"] == "night" and state["preferences"]["language"] == "en",
                     "Copied app restores theme and language")
        checks.action("quit")
    finally:
        if pid and pid != original_pid and process_running(pid):
            last = json.loads((directory / "command.json").read_text())
            if last.get("action") != "quit":
                checks.action("quit")
        second_exit = wait_exit(launcher, pid)
    return {"checks": checks.results["checks"], "exits": [first_exit, second_exit]}


def credentials(bundle, directory, fixture):
    """Exercise encrypted local persistence across two Launch Services starts of the bundle."""
    exits = []
    try:
        for phase in ["exercise", "restore"]:
            launcher = launch(bundle, directory)
            check = CredentialAcceptance(directory, fixture)
            previous_pid = check.state()["pid"] if (directory / "state.json").exists() else None
            pid = None
            try:
                state = check.wait(lambda value: value["pid"] != previous_pid, "Bundled process started through Launch Services")
                pid = state["pid"]
                getattr(check, phase)()
            finally:
                if pid and process_running(pid):
                    last = json.loads((directory / "command.json").read_text())
                    if last.get("action") != "quit":
                        check.action("dismiss")
                        check.action("quit")
                if pid:
                    exits.append(wait_exit(launcher, pid))
    finally:
        report = directory / "credential-results.json"
        if report.exists():
            record = json.loads(report.read_text())
            profile = record.get("profile")
            if profile:
                assert profile["name"] == "Credential persistence fixture" and profile["host"] in ["127.0.0.1", "::1", "localhost"]
                record["test_vault"] = "retained in isolated data directory; no keychain access"
                record["launch_services_exits"] = exits
                report.write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n")
    return json.loads((directory / "credential-results.json").read_text())


def main():
    """Keep the tested copy, reports and all runtime state in one new temporary directory."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--expected-version", help="Optional exact version of the supplied bundle")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("This runner requires macOS")
    bundle = args.bundle.resolve()
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=False)
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    assert info["CFBundleName"] == info["CFBundleDisplayName"] == "MantaSH"
    assert info["CFBundleIdentifier"] == "app.mantash.MantaSH"
    version = info["CFBundleShortVersionString"]
    assert re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version)
    assert version == info["CFBundleVersion"]
    if args.expected_version:
        assert version == args.expected_version
    assert not any(path.is_symlink() for path in bundle.rglob("*"))
    copied = root / "Finder Launch" / "MantaSH.app"
    copied.parent.mkdir()
    shutil.copytree(bundle, copied)
    subprocess.run(["/usr/bin/codesign", "--verify", "--strict", "--verbose=2", str(copied)], check=True)
    report = {"bundle": str(bundle), "info": info, "runtime": binary_metadata(copied / "Contents/MacOS/mantash"),
              "binary_sha256": digest(copied / "Contents/MacOS/mantash"),
              "launch_method": "macOS Launch Services via open -a; same service used by Finder, no physical double-click injected",
              "copied_outside_source": True, "symlinks": 0}
    path = root / "bundle-results.json"
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    report["native_smoke"] = smoke(copied, root / "smoke")
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    if args.fixture:
        report["credentials"] = credentials(copied, root / "credentials", args.fixture.resolve())
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"report": str(path), "native_smoke_checks": len(report["native_smoke"]["checks"]),
                      "credential_checks": len(report.get("credentials", {}).get("checks", []))}))


if __name__ == "__main__":
    main()
