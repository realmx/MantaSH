#!/usr/bin/env python3
"""Check the real update prompt and cancellation in an isolated macOS debug window.

Requires network access and a binary older than the latest stable GitHub Release.
Never confirms a download or invokes installation.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

from qa_overview_layout_macos import Driver


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {**os.environ, "MANTASH_DATA_DIR": str(root / "data"),
           "MANTASH_QA_CONTROL": str(root / "command.json")}
    env.pop("MANTASH_QA_BACKGROUND", None)
    with (root / "stdout.log").open("w") as stdout, (root / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=root, env=env,
                                   stdout=stdout, stderr=stderr)
        driver = Driver(root, process)
        try:
            driver.wait(lambda state: bool(state["tabs"]), "native startup")
            driver.action("about")
            driver.wait(lambda state: state["modal"] == "about", "About opens")
            driver.action("check_updates")
            state = driver.wait(lambda state: state["update"]["phase"] == "Available",
                                "a newer official stable release (requires an older binary and network)")
            version = state["update"]["version"]
            assert state["modal"] == "about", state
            assert state["update"]["downloaded"] == 0 and not state["update"]["prepared"], state
            driver.action("keystroke", key="escape")
            state = driver.wait(lambda state: state["modal"] == "update", "deferred update prompt")
            assert state["update"]["phase"] == "Available", state
            driver.action("keystroke", key="escape")
            state = driver.wait(lambda state: state["modal"] is None
                                and state["update"]["phase"] == "Idle", "update cancellation")
            assert version in state["update"]["declined"], state
            assert not state["update"]["request_active"], state
            assert state["update"]["downloaded"] == 0 and not state["update"]["prepared"], state
            driver.action("check_updates")
            state = driver.wait(lambda state: state["update"]["phase"] == "Idle"
                                and not state["update"]["request_active"], "same-version check completes")
            assert state["modal"] is None and version in state["update"]["declined"], state
            driver.action("about")
            driver.wait(lambda state: state["modal"] == "about", "About reopens")
            driver.action("check_updates", manual=True)
            state = driver.wait(lambda state: state["modal"] == "update"
                                and state["update"]["phase"] == "Available"
                                and state["update"]["manual"], "explicit check offers previously declined version")
            assert state["update"]["version"] == version, state
            assert state["update"]["downloaded"] == 0, state
            driver.action("keystroke", key="escape")
            state = driver.wait(lambda state: state["modal"] == "about"
                                and state["update"]["phase"] == "Idle", "manual check returns to About")
            assert not state["update"]["request_active"], state
            report = {"offered_version": version, "existing_modal_preserved": True,
                      "cancelled_without_download": True, "same_version_suppressed": True,
                      "manual_check_offers_declined_version": True, "manual_cancel_returns_to_about": True,
                      "scope": "native window with programmatic Esc; official metadata only; no installation"}
            (root / "updates-results.json").write_text(
                json.dumps(report, ensure_ascii=False, indent=2) + "\n")
            print(json.dumps(report, ensure_ascii=False))
        finally:
            if process.poll() is None:
                driver.send("quit")
            assert process.wait(timeout=20) == 0


if __name__ == "__main__":
    main()
