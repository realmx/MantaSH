#!/usr/bin/env python3
"""Validate repository-local Markdown links, sample metadata and required documentation."""
import csv
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    """Check concrete files and examples, without fetching websites or reading user configuration."""
    required = ["README.md", "README.en.md", "AGENTS.md", "LICENSE", "CONTRIBUTING.md", "CHANGELOG.md", "THIRD_PARTY.md"]
    required += [f"docs/{name}" for name in ("README.md", "features.md", "user-guide.md", "macos.md", "windows.md", "architecture.md", "implementation.md", "development.md", "ai-development.md", "release.md", "data.md", "design.md", "brand-philosophy.md", "terminal-reference.md", "delivery-status.md", "acceptance.md", "troubleshooting.md", "dependency-licenses.csv")]
    errors = []
    for name in required:
        path = ROOT / name
        if not path.is_file() or path.stat().st_size == 0:
            errors.append(f"Missing or empty document: {name}")
    checked = 0
    documents = list(ROOT.glob("*.md")) + list((ROOT / "docs").glob("*.md"))
    for document in documents:
        text = re.sub(r"```.*?```", "", document.read_text(encoding="utf-8"), flags=re.S)
        for link in re.findall(r"\]\(([^)]+)\)", text):
            target = link.strip().strip("<>").split("#", 1)[0]
            if not target or re.match(r"^[a-z]+:", target, flags=re.I):
                continue
            checked += 1
            if not (document.parent / target).resolve().exists():
                errors.append(f"Broken link in {document.relative_to(ROOT)}: {link}")
    with (ROOT / "examples/connections.csv").open(newline="") as source:
        reader = csv.reader(source)
        header = next(reader)
        rows = list(reader)
    # Import/export use only the five-column CSV format; examples carry no secrets.
    assert header == ["name", "host", "port", "username", "password"]
    assert rows and all(len(row) == 5 and row[4] == "" for row in rows)
    assert "GNU GENERAL PUBLIC LICENSE" in (ROOT / "LICENSE").read_text()
    # Undefined labels are visible defects in screenshots as well as documentation drift.
    catalog = (ROOT / "src/ui/i18n.rs").read_text()
    keys = set(re.findall(r'^\s*"([a-z_]+)"\s*=>', catalog, re.M))
    for source in (ROOT / "src/ui").glob("*.rs"):
        for key in set(re.findall(r'\bt\("([a-z_]+)"\)', source.read_text())) - keys:
            errors.append(f"Undefined UI label in {source.relative_to(ROOT)}: {key}")
    retired_keys = {"memory_details", "auth", "key", "search_terminal", "next", "previous_match"}
    for key in retired_keys:
        if re.search(rf'^\s*"{re.escape(key)}"\s*=>', catalog, re.M):
            errors.append(f"Retired UI label remains in catalog: {key}")
    obsolete_by_document = {
        "README.en.md": ("private key", "Files, Editor, History and System"),
        "docs/user-guide.md": ("三个标签", "私钥", "密钥路径", "勾选“记住密码”", "不包括密码"),
        "docs/data.md": ("SystemSecretStore", "勾选“记住密码”"),
        "docs/architecture.md": ("SystemSecretStore", "加密私钥"),
        "docs/development.md": ("SystemSecretStore", "私钥", "charts.js", "浏览器预览"),
        "docs/troubleshooting.md": ("私钥", "密钥路径", "勾选“记住密码”"),
        "docs/windows.md": ("私钥",),
        "docs/design.md": ("浏览器预览", "弹窗宽度 600px"),
    }
    for name, phrases in obsolete_by_document.items():
        text = (ROOT / name).read_text(encoding="utf-8")
        for phrase in phrases:
            if phrase in text:
                errors.append(f"Obsolete product wording in {name}: {phrase}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        raise SystemExit(1)
    print(f"Verified {len(required)} required documents, {checked} local links, matching CSV examples and literal UI label keys.")


if __name__ == "__main__":
    main()
