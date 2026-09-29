#!/usr/bin/env python3
"""Package a Windows installer or macOS disk image for a release build."""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BUNDLE_ID = "app.mantash.MantaSH"
DISPLAY_NAME = "MantaSH"
TARGETS = {
    "aarch64-apple-darwin": ("macos-arm64", "macos", None),
    "x86_64-apple-darwin": ("macos-x64", "macos", None),
    "i686-pc-windows-msvc": ("windows-x86", "windows", "x86compatible"),
    "x86_64-pc-windows-msvc": ("windows-x64", "windows", "x64compatible"),
    "aarch64-pc-windows-msvc": ("windows-arm64", "windows", "arm64"),
}
NOTICE_FILES = [
    "LICENSE",
    "THIRD_PARTY.md",
    "docs/dependency-licenses.csv",
    "assets/LUCIDE-LICENSE",
]


def digest(path: Path) -> str:
    """Hash a package without loading it all into memory."""
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def write_checksum(path: Path) -> str:
    checksum = digest(path)
    path.with_name(path.name + ".sha256").write_bytes(
        f"{checksum}  {path.name}\n".encode("ascii")
    )
    return checksum


def copy_file(source: Path, destination: Path) -> None:
    if source.is_symlink() or not source.is_file():
        raise RuntimeError(f"Missing or linked release input: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def write_notices(root: Path) -> None:
    for name in NOTICE_FILES:
        copy_file(ROOT / name, root / "Notices" / name)


def macos_app(stage: Path, binary: Path, version: str, target: str) -> Path:
    """Assemble an ad-hoc signed, unnotarized application bundle."""
    app = stage / "MantaSH.app"
    contents = app / "Contents"
    executable = contents / "MacOS/mantash"
    resources = contents / "Resources"
    copy_file(binary, executable)
    executable.chmod(0o755)
    copy_file(ROOT / "assets/mantash.icns", resources / "mantash.icns")
    write_notices(resources)
    information = {
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundlePackageType": "APPL",
        "CFBundleIdentifier": BUNDLE_ID,
        "CFBundleName": DISPLAY_NAME,
        "CFBundleDisplayName": DISPLAY_NAME,
        "CFBundleExecutable": "mantash",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "CFBundleIconFile": "mantash.icns",
        "CFBundleDevelopmentRegion": "en",
        "CFBundleLocalizations": ["en", "zh-Hans"],
        "LSApplicationCategoryType": "public.app-category.developer-tools",
        "LSMinimumSystemVersion": "13.0",
        "NSHighResolutionCapable": True,
        "NSSupportsAutomaticGraphicsSwitching": True,
        "NSLocalNetworkUsageDescription": "Connect to the local SSH servers you select.",
        "NSHumanReadableCopyright": "MantaSH contributors. GPL-3.0-or-later.",
    }
    contents.mkdir(parents=True, exist_ok=True)
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump(information, stream, sort_keys=True)
    (contents / "PkgInfo").write_bytes(b"APPL????")
    manifest = {
        "product": DISPLAY_NAME,
        "version": version,
        "target": target,
        "bundle_identifier": BUNDLE_ID,
        "profile": "release",
        "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source_revision": os.environ.get("GITHUB_SHA"),
        "binary_sha256": digest(executable),
        "signing": "Ad-hoc integrity signature; not Developer ID signed or Apple notarized.",
    }
    (resources / "build-info.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    subprocess.run(["/usr/bin/plutil", "-lint", str(contents / "Info.plist")], check=True)
    subprocess.run(["/usr/bin/codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    return app


def validate_windows_gui(binary: Path) -> None:
    """Reject console executables before packaging; PE32 and PE32+ share this field."""
    with binary.open("rb") as source:
        dos = source.read(64)
        if len(dos) != 64 or dos[:2] != b"MZ":
            raise ValueError(f"Invalid Windows executable: {binary}")
        pe_offset = int.from_bytes(dos[60:64], "little")
        if pe_offset < 64:
            raise ValueError(f"Invalid PE header offset: {binary}")
        source.seek(pe_offset)
        header = source.read(24)
        if len(header) != 24 or header[:4] != b"PE\0\0":
            raise ValueError(f"Invalid PE header: {binary}")
        optional_size = int.from_bytes(header[20:22], "little")
        optional = source.read(optional_size)
        if (optional_size < 70 or len(optional) != optional_size
                or int.from_bytes(optional[:2], "little") not in (0x10B, 0x20B)):
            raise ValueError(f"Invalid PE optional header: {binary}")
        subsystem = int.from_bytes(optional[68:70], "little")
        if subsystem != 2:  # IMAGE_SUBSYSTEM_WINDOWS_GUI
            raise ValueError(f"MantaSH must use Windows GUI subsystem 2, got {subsystem}: {binary}")


def validate_windows_icon(binary: Path) -> None:
    """On the Windows runner, verify the exact icon resource GPUI loads from the EXE."""
    if os.name != "nt":
        return
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.LoadLibraryExW.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]
    kernel32.LoadLibraryExW.restype = wintypes.HMODULE
    kernel32.FindResourceW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p]
    kernel32.FindResourceW.restype = ctypes.c_void_p
    kernel32.FreeLibrary.argtypes = [wintypes.HMODULE]
    kernel32.FreeLibrary.restype = wintypes.BOOL
    # Map resources only: never execute the binary while validating the package.
    module = kernel32.LoadLibraryExW(str(binary.resolve()), None, 0x02 | 0x20)
    if not module:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        if not kernel32.FindResourceW(module, 1, 14):  # ID 1, RT_GROUP_ICON
            raise ValueError(f"MantaSH executable is missing application icon resource 1: {binary}")
    finally:
        kernel32.FreeLibrary(module)


def windows_tree(stage: Path, binary: Path, version: str, target: str) -> Path:
    """Prepare the executable and notices for the Windows installer."""
    validate_windows_gui(binary)
    validate_windows_icon(binary)
    root = stage / "MantaSH"
    executable = root / "mantash.exe"
    copy_file(binary, executable)
    copy_file(ROOT / "assets/mantash.ico", root / "mantash.ico")
    write_notices(root)
    (root / "README.txt").write_text(
        "MantaSH " + version + "\n\n"
        "Run the installer to add Start menu and uninstall entries.\n\n"
        "Target: " + target + "\n"
        "License: GPL-3.0-or-later.\n",
        encoding="utf-8",
    )
    (root / "build-info.json").write_text(
        json.dumps({
            "product": DISPLAY_NAME,
            "version": version,
            "target": target,
            "profile": "release",
            "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "source_revision": os.environ.get("GITHUB_SHA"),
            "binary_sha256": digest(executable),
            "signing": "Unsigned Windows binary; SmartScreen may require explicit user approval.",
        }, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return root



def inno_compiler() -> str:
    compiler = shutil.which("ISCC.exe")
    if compiler:
        return compiler
    return str(Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Inno Setup 6/ISCC.exe")


def package_windows(stage: Path, binary: Path, version: str, target: str,
                    arch: str, basename: str, output_dir: Path) -> Path:
    root = windows_tree(stage, binary, version, target)
    installer = output_dir / f"{basename}-setup.exe"
    subprocess.run([
        inno_compiler(), f"/DAppVersion={version}", f"/DBuildArch={arch}",
        f"/DSourceDir={root.resolve()}", f"/DOutputDir={output_dir.resolve()}",
        f"/DOutputBase={installer.stem}", str(ROOT / "packaging/windows/MantaSH.iss"),
    ], check=True)
    if not installer.is_file():
        raise RuntimeError(f"Inno Setup did not create {installer}")
    return installer


def package_macos(stage: Path, binary: Path, version: str, target: str,
                  basename: str, output_dir: Path) -> Path:
    app = macos_app(stage, binary, version, target)
    dmg = output_dir / f"{basename}.dmg"
    dmg_root = stage / "dmg-root"
    dmg_root.mkdir()
    shutil.copytree(app, dmg_root / app.name)
    (dmg_root / "Applications").symlink_to("/Applications", target_is_directory=True)
    subprocess.run([
        "/usr/bin/hdiutil", "create", "-volname", DISPLAY_NAME,
        "-srcfolder", str(dmg_root), "-ov", "-format", "UDZO", str(dmg),
    ], check=True)
    return dmg


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=sorted(TARGETS), required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True, help="Numeric Cargo and app version")
    parser.add_argument("--package-version", help="Optional dev version for artifact filenames")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    if not args.binary.is_file():
        parser.error(f"Binary does not exist: {args.binary}")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.version):
        parser.error("--version must be a numeric X.Y.Z version")
    package_version = args.package_version or args.version
    if not re.fullmatch(re.escape(args.version) + r"(?:-dev\.[a-f0-9]{7})?", package_version):
        parser.error("--package-version must match --version or end with -dev.<seven hex digits>")
    name, kind, arch = TARGETS[args.target]
    if sys.platform != {"macos": "darwin", "windows": "win32"}[kind]:
        parser.error(f"{name} packages must be assembled on {kind}")
    basename = f"MantaSH-{package_version}-{name}"
    suffix = ".dmg" if kind == "macos" else "-setup.exe"
    output = args.output_dir / f"{basename}{suffix}"
    if output.exists() or output.with_name(output.name + ".sha256").exists():
        parser.error(f"Output already exists: {output}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="mantash-package-") as directory:
        stage = Path(directory)
        if kind == "macos":
            package = package_macos(stage, args.binary, args.version, args.target,
                                    basename, args.output_dir)
        else:
            package = package_windows(stage, args.binary, args.version, args.target,
                                      arch, basename, args.output_dir)
    print(json.dumps({str(package): write_checksum(package)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
