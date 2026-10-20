#!/usr/bin/env python3
"""Rebuild only the managed commit-log section of CHANGELOG.md from Git history.

The repository pre-commit hook (.githooks/pre-commit) runs the ``hook`` command,
which regenerates the section from HEAD (the parent of the commit being created)
and stages CHANGELOG.md as part of that same commit. A commit cannot contain its
own hash, so each commit appears in the log written by the next commit.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
from tempfile import NamedTemporaryFile

from check_docs import prose_lines

ROOT = Path(__file__).resolve().parents[1]
BEGIN = "<!-- BEGIN AUTO-GENERATED COMMIT LOG -->"
END = "<!-- END AUTO-GENERATED COMMIT LOG -->"
# Retired GitHub Actions sync commits carried this trailer; they stay excluded.
TRAILER = "MantaSH-Changelog: generated"
SKIP_ENV = "MANTASH_SKIP_CHANGELOG"
COMMIT_URL = "https://github.com/realmx/MantaSH/commit/"


@dataclass(frozen=True)
class Commit:
    """Public commit metadata; no author addresses or working files are inspected."""

    sha: str
    date: str
    subject: str
    body: str


def git(root: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    """Pass arguments directly to Git, with bounded, non-interactive operations."""
    result = subprocess.run(
        ["git", "-C", str(root), *args], capture_output=True, text=True,
        encoding="utf-8", env={**os.environ, "GIT_TERMINAL_PROMPT": "0"}, timeout=60,
    )
    if check and result.returncode:
        raise RuntimeError(f"Git {args[0]} failed: {result.stderr.strip() or result.stdout.strip()}")
    return result


def generated_commit(root: Path, commit: Commit) -> bool:
    """Exclude only a marked, single-parent commit changing exactly the log.

    Such commits were created by the retired Actions publisher; keeping the rule
    keeps the regenerated log identical for the history that already has them.
    """
    if commit.body.rstrip().splitlines()[-1:] != [TRAILER]:
        return False
    if len(git(root, "rev-list", "--parents", "-1", commit.sha).stdout.split()) != 2:
        return False
    paths = git(
        root, "diff-tree", "--no-commit-id", "--no-renames", "--name-only", "-z",
        "-r", commit.sha, "--",
    ).stdout.split("\0")
    return [path for path in paths if path] == ["CHANGELOG.md"]


def commits(root: Path, revision: str = "HEAD") -> list[Commit]:
    """Include every reachable ordinary, merge and empty commit in stable order."""
    if git(root, "rev-parse", "--is-shallow-repository").stdout.strip() == "true":
        raise ValueError("A complete Git history is required; fetch with --unshallow first.")
    if git(root, "rev-parse", "--verify", "--quiet", "HEAD", check=False).returncode:
        return []
    fields = git(
        root, "log", "--topo-order", "-z", "--format=%H%x00%cs%x00%s%x00%b", revision, "--",
    ).stdout.split("\0")
    if fields.pop() != "" or len(fields) % 4:
        raise ValueError("Unexpected Git commit metadata format")
    result = []
    for offset in range(0, len(fields), 4):
        commit = Commit(*fields[offset:offset + 4])
        if not generated_commit(root, commit):
            result.append(commit)
    return result


def literal_subject(subject: str) -> str:
    """Render commit titles as text, including Markdown/HTML-looking titles."""
    return "".join(
        f"&#{ord(character)};" if character in "\\`*_[]<>&" or ord(character) < 32 else character
        for character in subject
    )


def commit_section(history: list[Commit]) -> str:
    """Keep full bodies inside fences that their own text cannot terminate."""
    lines = [
        BEGIN, "", "## 逐次提交记录（自动生成）", "",
        "本区由提交前钩子按 Git 完整历史自动生成，随每次提交写入，覆盖到该提交的上一提交（最新一条在下次提交时补入）；包含普通提交、合并和文档更新，仅省略旧版 Actions 机器人的日志同步提交。日期为提交日期，正文保留原始说明，不代表对应版本已经发布或通过平台验收。", "",
    ]
    previous_date = None
    for commit in history:
        if commit.date != previous_date:
            lines.extend([f"### {commit.date}", ""])
            previous_date = commit.date
        lines.extend([
            f"- **{literal_subject(commit.subject) or '（无标题）'}** · [{commit.sha[:7]}]({COMMIT_URL}{commit.sha})", "",
        ])
        body = commit.body.rstrip("\n")
        if body:
            longest = max((len(match[0]) for match in re.finditer(r"`+", body)), default=0)
            fence = "`" * max(3, longest + 1)
            lines.extend(["<details>", "<summary>完整提交说明</summary>", "", fence + "text", body, fence, "", "</details>", ""])
    if not history:
        lines.extend(["尚无提交记录。", ""])
    lines.append(END)
    return "\n".join(lines)


def managed_markers(original: str) -> list[tuple[str, int, int]]:
    """Locate BEGIN/END marker lines outside fenced code as (marker, start, end)."""
    markers = []
    offset = 0
    for raw, prose in zip(original.splitlines(keepends=True), prose_lines(original)):
        if prose in (BEGIN, END):
            markers.append((prose, offset, offset + len(raw.rstrip("\r\n"))))
        offset += len(raw)
    if markers and [marker[0] for marker in markers] != [BEGIN, END]:
        raise ValueError("Invalid or duplicate CHANGELOG managed markers; no file was changed.")
    return markers


def managed_region(original: str) -> str | None:
    """Return the generated section (markers included), or None when absent."""
    markers = managed_markers(original)
    return original[markers[0][1]:markers[1][2]] if markers else None


def render(original: str, history: list[Commit]) -> str:
    """Replace only valid markers outside fenced code; preserve curated prose."""
    markers = managed_markers(original)
    section = commit_section(history)
    if not markers:
        separator = "" if original.endswith("\n\n") else "\n" if original.endswith("\n") else "\n\n"
        return original + separator + section + "\n"
    return original[:markers[0][1]] + section + original[markers[1][2]:]


def read_log(path: Path) -> str:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Expected an existing regular changelog: {path}")
    return path.read_bytes().decode("utf-8")


def write_log(path: Path, text: str, expected: str | None = None) -> None:
    """Atomically replace one output while guarding concurrent manual edits."""
    if path.is_symlink() or (path.exists() and not path.is_file()):
        raise ValueError(f"Refusing a non-regular changelog output: {path}")
    mode = stat.S_IMODE(path.stat().st_mode) if path.exists() else 0o644
    temporary = None
    try:
        with NamedTemporaryFile(dir=path.parent, prefix=".changelog-", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(text.encode("utf-8"))
            stream.flush()
            os.fsync(stream.fileno())
        if expected is not None and read_log(path) != expected:
            raise ValueError("CHANGELOG.md changed during generation; no file was replaced.")
        temporary.chmod(mode)
        temporary.replace(path)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def update(root: Path, output: Path | None = None, check: bool = False) -> bool:
    """Generate from HEAD without altering the index or any curated summaries."""
    path = root / "CHANGELOG.md"
    original = read_log(path)
    history = commits(root)
    text = render(original, history)
    changed = text != original
    if check and changed:
        raise ValueError("CHANGELOG.md is stale; run python3 scripts/update_changelog.py generate.")
    if not check and (changed or output is not None):
        write_log(output or path, text, expected=original if output is None else None)
    print(f"Changelog: {len(history)} commits; {'updated' if changed else 'current'}.")
    return changed


def partial_commit_index(root: Path) -> bool:
    """True for `git commit <paths>`, which commits from a temporary index.

    Staging into that index would put the log into the commit while the real
    index keeps the old blob, leaving a confusing reverse change behind.
    """
    index = os.environ.get("GIT_INDEX_FILE")
    if not index:
        return False
    # --git-path honours GIT_INDEX_FILE, so ask for the default without it.
    default = Path(subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--path-format=absolute", "--git-path", "index"],
        capture_output=True, text=True, check=True, timeout=60,
        env={key: value for key, value in os.environ.items() if key != "GIT_INDEX_FILE"},
    ).stdout.strip())
    current = Path(index) if os.path.isabs(index) else root / index
    return current.resolve() not in (default.resolve(), Path(str(default) + ".lock").resolve())


def hook(root: Path, amend: bool = False) -> bool:
    """pre-commit: regenerate the log from HEAD and stage it into this commit.

    Returns True when CHANGELOG.md was restaged. Situations where the log cannot
    be generated safely are skipped with a notice instead of blocking the commit;
    the next commit regenerates the complete log anyway. Only CHANGELOG.md is
    ever staged. When amending, HEAD is about to be replaced, so the log covers
    the parents of HEAD instead of HEAD itself. With unstaged CHANGELOG.md
    edits, only the generated section is replaced in both the index and the
    working file, so the user's unstaged edits stay unstaged; hand edits inside
    the generated section abort instead.
    """
    if os.environ.get(SKIP_ENV) == "1":
        print(f"Changelog: skipped ({SKIP_ENV}=1).")
        return False
    if git(root, "rev-parse", "--verify", "--quiet", "HEAD", check=False).returncode:
        print("Changelog: skipped for the initial commit; the next commit records it.")
        return False
    if git(root, "rev-parse", "--is-shallow-repository").stdout.strip() == "true":
        print("Warning: shallow clone; changelog not updated. Run git fetch --unshallow.", file=sys.stderr)
        return False
    if partial_commit_index(root):
        print("Warning: git commit <paths> uses a temporary index; changelog not updated "
              "in this commit. The next commit records it.", file=sys.stderr)
        return False
    entry = git(root, "ls-files", "--stage", "--", "CHANGELOG.md").stdout.split()
    if not entry:
        print("Changelog: CHANGELOG.md is not tracked; skipped.")
        return False
    mode, blob = entry[0], entry[1]
    staged = subprocess.run(
        ["git", "-C", str(root), "cat-file", "blob", blob], capture_output=True, check=True, timeout=60,
    ).stdout.decode("utf-8")
    revision = "HEAD"
    if amend:
        if len(git(root, "rev-list", "--parents", "-1", "HEAD").stdout.split()) < 2:
            print("Changelog: skipped while amending the initial commit.")
            return False
        revision = "HEAD^@"
    history = commits(root, revision)
    staged_text = render(staged, history)
    if staged_text == staged:
        print(f"Changelog: {len(history)} commits; current.")
        return False
    path = root / "CHANGELOG.md"
    working = read_log(path)
    if git(root, "diff", "--quiet", "--", "CHANGELOG.md", check=False).returncode == 0:
        write_log(path, render(working, history), expected=working)
        git(root, "add", "--", "CHANGELOG.md")
    else:
        normalized = [
            region.replace("\r\n", "\n") if region is not None else None
            for region in (managed_region(staged), managed_region(working))
        ]
        if normalized[0] != normalized[1]:
            raise ValueError(
                "CHANGELOG.md has unstaged edits inside the generated commit log; "
                "stage, stash or discard them, or commit with " + SKIP_ENV + "=1."
            )
        stored = subprocess.run(
            ["git", "-C", str(root), "hash-object", "-w", "--no-filters", "--stdin"],
            input=staged_text.encode("utf-8"), capture_output=True, check=True, timeout=60,
        ).stdout.decode().strip()
        git(root, "update-index", "--cacheinfo", f"{mode},{stored},CHANGELOG.md")
        write_log(path, render(working, history), expected=working)
    print(f"Changelog: {len(history)} commits; staged CHANGELOG.md.")
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("generate", "check", "hook"))
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, help="Write a preview instead of replacing CHANGELOG.md")
    parser.add_argument("--amend", action="store_true", help="hook only: the commit replaces HEAD")
    args = parser.parse_args(argv)
    if args.output is not None and args.command != "generate":
        parser.error("--output is only valid with generate")
    if args.amend and args.command != "hook":
        parser.error("--amend is only valid with hook")
    try:
        root = args.root.resolve()
        if args.command == "hook":
            hook(root, args.amend)
        else:
            update(root, args.output, check=args.command == "check")
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
