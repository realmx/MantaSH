#!/usr/bin/env python3
"""Write a release body containing the commits since the previous version tag."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess


def git(*args: str) -> str:
    """Run a repository-local Git query and return its text output."""
    return subprocess.check_output(["git", *args], text=True).strip()


def previous_tag(commit: str, current_tag: str) -> str | None:
    """Find the latest reachable version tag before the current release tag."""
    tags = git("tag", "--merged", commit, "--sort=-version:refname").splitlines()
    return next((tag for tag in tags if tag.startswith("v") and tag != current_tag), None)


def subjects(commit: str, start: str | None) -> list[str]:
    """Collect ordinary commits, including work merged from a pull request."""
    revision = f"{start}..{commit}" if start else commit
    return [value for value in git(
        "log", "--no-merges", "--format=%s", "--reverse", revision
    ).splitlines() if value]


def main() -> int:
    """Generate a stable Markdown summary for the GitHub Release body."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", default=None)
    args = parser.parse_args()

    commit = args.commit or git("rev-parse", "HEAD")
    current_tag = f"v{args.version}"
    start = previous_tag(commit, current_tag)
    changes = subjects(commit, start)
    if not changes:
        changes = ["No user-facing commit subjects were found for this release."]

    lines = [f"# MantaSH v{args.version}", "", "## Changes", ""]
    lines.extend(f"- {change}" for change in changes)
    lines.extend([
        "", "## macOS installation", "",
        "DMGs are ad-hoc signed and not Apple notarized. macOS Gatekeeper may block first launch; verify the download and use the system Open Anyway control.",
        "", "## Artifacts", "",
        "- macOS arm64 and x64 DMGs",
        "- Windows x86, x64 and arm64 unsigned installers (SmartScreen may warn)",
        "- SHA-256 checksums for every package",
    ])
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
