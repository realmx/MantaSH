#!/usr/bin/env python3
"""Rebuild the commit log and publish only its managed CHANGELOG.md section."""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
from tempfile import NamedTemporaryFile, TemporaryDirectory

from check_docs import prose_lines

ROOT = Path(__file__).resolve().parents[1]
BEGIN = "<!-- BEGIN AUTO-GENERATED COMMIT LOG -->"
END = "<!-- END AUTO-GENERATED COMMIT LOG -->"
TRAILER = "MantaSH-Changelog: generated"
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
    """Exclude only a marked, single-parent commit changing exactly the log."""
    if commit.body.rstrip().splitlines()[-1:] != [TRAILER]:
        return False
    if len(git(root, "rev-list", "--parents", "-1", commit.sha).stdout.split()) != 2:
        return False
    paths = git(
        root, "diff-tree", "--no-commit-id", "--no-renames", "--name-only", "-z",
        "-r", commit.sha, "--",
    ).stdout.split("\0")
    return [path for path in paths if path] == ["CHANGELOG.md"]


def commits(root: Path) -> list[Commit]:
    """Include every reachable ordinary, merge and empty commit in stable order."""
    if git(root, "rev-parse", "--is-shallow-repository").stdout.strip() == "true":
        raise ValueError("A complete Git history is required; fetch with --unshallow first.")
    if git(root, "rev-parse", "--verify", "--quiet", "HEAD", check=False).returncode:
        return []
    fields = git(
        root, "log", "--topo-order", "-z", "--format=%H%x00%cs%x00%s%x00%b", "HEAD", "--",
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
        "本区按 Git 完整历史自动生成，包含普通提交、合并和文档更新；仅省略机器人自身的日志同步提交。日期为提交日期，正文保留原始说明，不代表对应版本已经发布或通过平台验收。", "",
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


def render(original: str, history: list[Commit]) -> str:
    """Replace only valid markers outside fenced code; preserve curated prose."""
    markers = []
    offset = 0
    for raw, prose in zip(original.splitlines(keepends=True), prose_lines(original)):
        if prose in (BEGIN, END):
            markers.append((prose, offset, offset + len(raw.rstrip("\r\n"))))
        offset += len(raw)
    section = commit_section(history)
    if not markers:
        separator = "" if original.endswith("\n\n") else "\n" if original.endswith("\n") else "\n\n"
        return original + separator + section + "\n"
    if [marker[0] for marker in markers] != [BEGIN, END]:
        raise ValueError("Invalid or duplicate CHANGELOG managed markers; no file was changed.")
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


def validate_documents(root: Path) -> None:
    """Validate Markdown structure while preserving literal commit examples."""
    # Quoted commit bodies may legitimately contain trailing whitespace, mixed
    # indentation or conflict markers; git diff --check would reject that history.
    subprocess.run([sys.executable, str(root / "scripts/check_docs.py")], cwd=root, check=True, timeout=60)


def publish(root: Path, attempts: int = 3) -> bool:
    """Regenerate in owned worktrees; retry advancing master without force-push."""
    for attempt in range(attempts):
        git(root, "fetch", "origin", "refs/heads/master")
        source = git(root, "rev-parse", "FETCH_HEAD").stdout.strip()
        with TemporaryDirectory(prefix="mantash-changelog-") as directory:
            checkout = Path(directory) / "checkout"
            git(root, "worktree", "add", "--quiet", "--detach", str(checkout), source)
            try:
                if not update(checkout):
                    print("Remote master already has a complete changelog.")
                    return False
                validate_documents(checkout)
                git(checkout, "add", "--", "CHANGELOG.md")
                staged = git(checkout, "diff", "--cached", "--name-only").stdout.splitlines()
                if staged != ["CHANGELOG.md"]:
                    raise ValueError("Changelog publication must stage only CHANGELOG.md")
                git(
                    checkout, "-c", "user.name=github-actions[bot]", "-c",
                    "user.email=41898282+github-actions[bot]@users.noreply.github.com",
                    "-c", "commit.gpgSign=false", "commit", "--quiet", "-m",
                    "docs: 自动同步更新日志", "-m",
                    "按 master 的完整 Git 历史更新逐次提交记录，保留人工维护的功能摘要。\n\n" + TRAILER,
                )
                result = git(checkout, "push", "origin", "HEAD:refs/heads/master", check=False)
                if result.returncode == 0:
                    print(f"Published changelog for {source}.")
                    return True
                git(root, "fetch", "origin", "refs/heads/master")
                current = git(root, "rev-parse", "FETCH_HEAD").stdout.strip()
                if current == source or attempt + 1 == attempts:
                    raise RuntimeError(f"Changelog push rejected: {result.stderr.strip() or result.stdout.strip()}")
                print("Remote master advanced; regenerating its complete history.")
            finally:
                # Only this invocation's temporary checkout is removed, never the caller's files.
                git(root, "worktree", "remove", "--force", str(checkout))
    raise RuntimeError("Changelog publication exhausted its attempts")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("generate", "check", "publish"))
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output", type=Path, help="Write a preview instead of replacing CHANGELOG.md")
    args = parser.parse_args(argv)
    if args.output is not None and args.command != "generate":
        parser.error("--output is only valid with generate")
    try:
        root = args.root.resolve()
        if args.command == "publish":
            publish(root)
        else:
            update(root, args.output, check=args.command == "check")
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
