#!/usr/bin/env python3
"""Select AShell-style release targets and package versions for GitHub Actions."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re

from release_version import parts, source_version

TARGETS = [
    {"name": "windows-x64", "os": "windows-2022", "target": "x86_64-pc-windows-msvc", "python": "python"},
    {"name": "windows-x86", "os": "windows-2022", "target": "i686-pc-windows-msvc", "python": "python"},
    {"name": "windows-arm64", "os": "windows-2022", "target": "aarch64-pc-windows-msvc", "python": "python"},
    {"name": "macos-arm64", "os": "macos-14", "target": "aarch64-apple-darwin", "python": "python3"},
    {"name": "macos-x64", "os": "macos-14", "target": "x86_64-apple-darwin", "python": "python3"},
]
PLATFORMS = ("all", "windows-x64", "macos-arm64")


def select(ref_type: str, ref_name: str, event: str, platform: str,
           sha: str, baseline: str) -> dict[str, str]:
    """Tag builds publish all targets; branch builds provide selectable dev artifacts."""
    if platform not in PLATFORMS or event not in ("push", "workflow_dispatch"):
        raise ValueError("Unsupported release trigger or platform")
    if not re.fullmatch(r"[a-fA-F0-9]{40}", sha):
        raise ValueError("Expected a full source commit SHA")
    if ref_type == "tag":
        if not ref_name.startswith("v"):
            raise ValueError("Release tag must be vX.Y.Z")
        version = ref_name[1:]
        if parts(version) < parts(baseline):
            raise ValueError("Release tag predates Cargo baseline")
        selected = TARGETS
        publish = True
        package_version = version
    elif ref_type == "branch":
        if event == "push" and not ref_name.startswith("build/"):
            raise ValueError("Only build/** branch pushes run development builds")
        version = baseline
        publish = False
        package_version = f"{baseline}-dev.{sha[:7].lower()}"
        if ref_name in ("build/windows-x64", "build/macos-arm64"):
            platform = ref_name[6:]
        selected = TARGETS if platform == "all" else [target for target in TARGETS if target["name"] == platform]
    else:
        raise ValueError("Release requires a branch or version tag ref")
    return {
        "version": version,
        "package_version": package_version,
        "publish": str(publish).lower(),
        "matrix": json.dumps({"include": selected}, separators=(",", ":")),
    }


def main() -> None:
    result = select(
        os.environ["GITHUB_REF_TYPE"], os.environ["GITHUB_REF_NAME"],
        os.environ["GITHUB_EVENT_NAME"], os.environ.get("INPUT_PLATFORM", "all"),
        os.environ["GITHUB_SHA"], source_version(),
    )
    if output := os.environ.get("GITHUB_OUTPUT"):
        with Path(output).open("a", encoding="utf-8") as stream:
            for key, value in result.items():
                stream.write(f"{key}={value}\n")
    print(f"{result['package_version']}: {', '.join(row['name'] for row in json.loads(result['matrix'])['include'])}")


if __name__ == "__main__":
    main()
