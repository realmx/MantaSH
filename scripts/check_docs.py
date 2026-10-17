#!/usr/bin/env python3
"""Validate local documentation links, heading anchors, licenses and UI label keys."""
import csv
import html
from pathlib import Path
import re
import sys
import unicodedata
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


def prose_lines(text):
    """Exclude fenced code so command examples do not define links or headings."""
    fence = None
    for line in text.splitlines():
        marker = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if fence is not None:
            if marker and marker[1][0] == fence[0] and len(marker[1]) >= len(fence) and not marker[2].strip():
                fence = None
            yield ""
        elif marker:
            fence = marker[1]
            yield ""
        else:
            yield line


def heading_anchors(text):
    """Generate GitHub-style IDs for the ATX/Setext headings used in our docs."""
    anchors = set()
    previous = ""
    for line in prose_lines(text):
        match = re.match(r"^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        heading = match[1] if match else None
        if heading is None and previous.strip() and re.fullmatch(r" {0,3}(?:=+|-+)\s*", line):
            heading = previous.strip()
        previous = line
        if heading is None:
            continue
        heading = re.sub(r"!?\[([^\]]+)\]\([^)]+\)", r"\1", heading)
        heading = html.unescape(re.sub(r"<[^>]+>", "", heading))
        slug = "".join(
            character for character in heading.lower()
            if character in "-_" or character.isspace()
            or unicodedata.category(character)[0] not in "PSCZ"
        ).replace(" ", "-").replace("\t", "-")
        anchor = slug
        suffix = 0
        while anchor in anchors:
            suffix += 1
            anchor = f"{slug}-{suffix}"
        anchors.add(anchor)
    return anchors


def check_links(documents, root):
    """Check repository-local inline links, including same-page heading fragments."""
    errors = []
    checked = 0
    anchors = {}
    link_pattern = re.compile(r'\]\(\s*(<[^>\n]+>|(?:\\.|[^\s)])+)(?:\s+(?:"[^"]*"|\x27[^\x27]*\x27))?\s*\)')
    for document in documents:
        text = "\n".join(prose_lines(document.read_text(encoding="utf-8")))
        for match in link_pattern.finditer(text):
            link = html.unescape(re.sub(r"\\([()])", r"\1", match[1].strip("<>")))
            url = urlsplit(link)
            if url.scheme or url.netloc:
                continue
            checked += 1
            target = (document.parent / unquote(url.path)).resolve() if url.path else document.resolve()
            label = f"{document.relative_to(root)}: {link}"
            try:
                target.relative_to(root)
            except ValueError:
                errors.append(f"Link leaves repository in {label}")
                continue
            if not target.exists():
                errors.append(f"Broken link in {label}")
            elif url.fragment and target.is_file() and target.suffix.lower() == ".md":
                if target not in anchors:
                    anchors[target] = heading_anchors(target.read_text(encoding="utf-8"))
                if unquote(url.fragment) not in anchors[target]:
                    errors.append(f"Broken heading anchor in {label}")
    return checked, errors


def main():
    """Check repository files without websites, user configuration or demo data."""
    required = ["README.md", "README.en.md", "AGENTS.md", "LICENSE", "CONTRIBUTING.md", "CHANGELOG.md", "THIRD_PARTY.md"]
    required += [f"docs/{name}" for name in (
        "README.md", "features.md", "user-guide.md", "updates.md", "macos.md", "windows.md",
        "architecture.md", "development.md", "release.md", "data.md", "design.md",
        "brand-philosophy.md", "acceptance.md", "troubleshooting.md", "dependency-licenses.csv",
    )]
    errors = []
    for name in required:
        path = ROOT / name
        if not path.is_file() or path.stat().st_size == 0:
            errors.append(f"Missing or empty document: {name}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        raise SystemExit(1)
    documents = sorted(ROOT.glob("*.md")) + sorted((ROOT / "docs").rglob("*.md"))
    checked, link_errors = check_links(documents, ROOT)
    errors.extend(link_errors)
    with (ROOT / "docs/dependency-licenses.csv").open(newline="", encoding="utf-8-sig") as source:
        reader = csv.reader(source)
        if next(reader, []) != ["package", "version", "license", "license_file", "repository", "source"]:
            errors.append("Invalid dependency license inventory header")
        rows = list(reader)
        if not rows or any(len(row) != 6 or not all(row[:3]) for row in rows):
            errors.append("Invalid or empty dependency license inventory")
    if "GNU GENERAL PUBLIC LICENSE" not in (ROOT / "LICENSE").read_text():
        errors.append("Missing GPL license text")
    # Undefined labels are visible UI defects as well as documentation drift.
    catalog = (ROOT / "src/ui/i18n.rs").read_text()
    keys = set(re.findall(r'^\s*"([a-z_]+)"\s*=>', catalog, re.M))
    for source in (ROOT / "src/ui").glob("*.rs"):
        for key in set(re.findall(r'\bt\("([a-z_]+)"\)', source.read_text())) - keys:
            errors.append(f"Undefined UI label in {source.relative_to(ROOT)}: {key}")
    retired_keys = {"memory_details", "auth", "key", "search_terminal", "next", "previous_match", "default_ssh_port", "remember_credentials_hint"}
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
    print(f"Verified {len(required)} required documents, {checked} local links and heading anchors, dependency licenses and literal UI label keys.")


if __name__ == "__main__":
    main()
