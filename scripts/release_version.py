#!/usr/bin/env python3
"""Assign immutable source tags and stage their package version on release runners."""
from __future__ import annotations

import argparse
import csv
import io
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r"^([0-9]+)\.([0-9]+)\.([0-9]+)$")
MANIFEST_VERSION = re.compile(r'(?m)^(version\s*=\s*")([0-9]+\.[0-9]+\.[0-9]+)("\s*)$')
LOCK_VERSION = re.compile(r'(?m)(\[\[package\]\]\r?\nname = "mantash"\r?\nversion = ")([0-9]+\.[0-9]+\.[0-9]+)(")')

def documentation_only(paths: list[str]) -> bool:
    """Skip tags only for known documentation and README screenshot paths."""
    return bool(paths) and all(
        (path.endswith(".md") and ("/" not in path or path.startswith("docs/")))
        or (path.startswith("assets/screenshots/") and path.lower().endswith(
            (".png", ".jpg", ".jpeg", ".webp")
        ))
        for path in paths
    )


def changed_paths_since(tag: str, commit: str, root: Path = ROOT) -> list[str]:
    """Compare complete trees; a rename includes both paths so source removals count."""
    output = subprocess.check_output(
        ["git", "diff", "--no-renames", "--name-only", "-z", tag, commit, "--"], cwd=root
    )
    return [os.fsdecode(path) for path in output.split(b"\0") if path]


def release_relevant_changes(tag: str, commit: str, root: Path = ROOT) -> bool:
    paths = changed_paths_since(tag, commit, root)
    return bool(paths) and not documentation_only(paths)


def parts(version: str) -> tuple[int, int, int]:
    """Accept only version tags with three numeric components."""
    match = VERSION.fullmatch(version)
    if not match:
        raise ValueError(f"Invalid release version: {version}")
    return tuple(map(int, match.groups()))


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).strip()


def source_version(root: Path = ROOT) -> str:
    """Read the checked-in package version without modifying the repository."""
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    matches = MANIFEST_VERSION.findall(text)
    if len(matches) != 1:
        raise ValueError("Expected one top-level Cargo package version")
    version = matches[0][1]
    parts(version)
    return version


def next_version(base: str, tags: list[str]) -> str:
    """Use the latest numbered tag, or a newer manually selected Cargo baseline."""
    baseline = parts(base)
    published = [parts(tag[1:]) for tag in tags if tag.startswith("v") and VERSION.fullmatch(tag[1:])]
    if not published or baseline > max(published):
        return base
    major, minor, patch = max(published)
    return f"{major}.{minor}.{patch + 1}"


def rewrite_one(path: Path, pattern: re.Pattern[str], before: str, after: str) -> str:
    text = path.read_text(encoding="utf-8")
    matches = list(pattern.finditer(text))
    if len(matches) != 1 or matches[0].group(2) != before:
        raise ValueError(f"Unexpected package version in {path}")
    match = matches[0]
    return text[:match.start(2)] + after + text[match.end(2):]


def staged_files(root: Path, version: str) -> dict[Path, str]:
    """Validate all staged metadata before touching any runner-local file."""
    baseline = source_version(root)
    if parts(version) < parts(baseline):
        raise ValueError(f"Release {version} predates checked-in Cargo baseline {baseline}")
    changes = {
        root / "Cargo.toml": rewrite_one(root / "Cargo.toml", MANIFEST_VERSION, baseline, version),
        root / "Cargo.lock": rewrite_one(root / "Cargo.lock", LOCK_VERSION, baseline, version),
    }
    licenses = root / "docs/dependency-licenses.csv"
    original = licenses.read_text(encoding="utf-8")
    rows = list(csv.reader(io.StringIO(original)))
    matches = [row for row in rows[1:] if row and row[0] == "mantash"]
    if len(matches) != 1 or matches[0][1] != baseline:
        raise ValueError(f"Unexpected MantaSH license metadata in {licenses}")
    matches[0][1] = version
    newline = "\r\n" if "\r\n" in original else "\n"
    buffer = io.StringIO(newline="")
    writer = csv.writer(buffer, lineterminator=newline)
    writer.writerows(rows)
    changes[licenses] = buffer.getvalue()
    return changes


def stage(root: Path, version: str) -> None:
    changes = staged_files(root, version)
    for path, text in changes.items():
        with path.open("w", encoding="utf-8", newline="") as stream:
            stream.write(text)


def create_tag(tag: str, commit: str) -> None:
    """Push an immutable ref using checkout credentials, then record it locally.

    Git receive-pack reports the server's rejection reason, unlike the generic
    HTTP 422 returned by the Git References API. Never force an existing tag or
    leave a local success marker when the remote rejected the update.
    """
    print(f"Creating refs/tags/{tag} at {commit}", flush=True)
    subprocess.run(
        ["git", "push", "--porcelain", "origin", f"{commit}:refs/tags/{tag}"],
        check=True,
    )
    git("tag", tag, commit)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("tag", "validate", "stage"))
    parser.add_argument("--version", help="Exact vX.Y.Z tag version for validate/stage")
    parser.add_argument("--root", type=Path, default=ROOT, help="Runner-local source directory for stage")
    args = parser.parse_args(argv)
    try:
        if args.command == "tag":
            if args.version:
                parser.error("tag calculates its own version")
            commit = git("rev-parse", "HEAD")
            existing = [tag for tag in git("tag", "--points-at", commit).splitlines()
                        if tag.startswith("v") and VERSION.fullmatch(tag[1:])]
            if existing:
                version, created = max(existing, key=lambda tag: parts(tag[1:]))[1:], False
            else:
                tags = [tag for tag in git("tag", "--merged", "origin/master").splitlines()
                        if tag.startswith("v") and VERSION.fullmatch(tag[1:])]
                latest = max(tags, key=lambda tag: parts(tag[1:])) if tags else None
                if latest and subprocess.run(
                    ["git", "merge-base", "--is-ancestor", latest, commit], check=False
                ).returncode != 0:
                    version, created = latest[1:], False
                    print(f"Skipping older push {commit}; newer version {latest} already exists")
                elif latest and not release_relevant_changes(latest, commit):
                    version, created = latest[1:], False
                    print(f"Skipping documentation-only or unchanged update {commit} since {latest}")
                else:
                    version = next_version(source_version(args.root), git("tag", "--list").splitlines())
                    tag = "v" + version
                    create_tag(tag, commit)
                    created = True
            print(f"Release version v{version} for {commit}: {'tag created' if created else 'no new tag'}")
            if output := os.environ.get("GITHUB_OUTPUT"):
                with Path(output).open("a", encoding="utf-8") as stream:
                    stream.write(f"version={version}\ncreated={str(created).lower()}\n")
        else:
            if args.version is None:
                parser.error("--version is required")
            parts(args.version)
            if args.command == "validate":
                if parts(args.version) < parts(source_version(args.root)):
                    raise ValueError("Release tag predates the source Cargo version")
                if os.environ.get("GITHUB_ACTIONS") == "true":
                    if os.environ.get("GITHUB_REF_TYPE") != "tag" or os.environ.get("GITHUB_REF_NAME") != "v" + args.version:
                        raise ValueError("Release must run from its matching vX.Y.Z tag")
                print(f"Validated MantaSH v{args.version}")
            else:
                stage(args.root, args.version)
                print(f"Staged MantaSH {args.version} in runner-local package metadata")
    except (ValueError, subprocess.CalledProcessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
