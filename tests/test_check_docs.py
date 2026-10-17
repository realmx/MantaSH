"""Regression checks for local Markdown navigation after document consolidation."""
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import check_docs


class DocumentationLinksTests(unittest.TestCase):
    def test_chinese_punctuation_code_and_duplicate_headings(self):
        text = "# 编码、在线保存与文件权限\n## `Owner` / 请求\n## 重复\n## 重复\n## 重复-1\n"
        self.assertEqual(check_docs.heading_anchors(text), {
            "编码在线保存与文件权限", "owner--请求", "重复", "重复-1", "重复-1-1",
        })

    def test_fenced_examples_do_not_define_headings(self):
        text = "# Real\n````md\n# Hidden\n```\n# Still hidden\n````\n~~~\n# Also hidden\n~~~\nSetext\n------\n"
        self.assertEqual(check_docs.heading_anchors(text), {"real", "setext"})

    def test_resolves_relative_encoded_and_same_page_anchors(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "guide space.md").write_text("# 设置\n", encoding="utf-8")
            (root / "asset.svg").write_text("<svg/>\n", encoding="utf-8")
            document = root / "README.md"
            document.write_text(
                '# Overview\n[local](#overview)\n[guide](guide%20space.md#%E8%AE%BE%E7%BD%AE)\n'
                '[title](<guide space.md#设置> "Guide")\n![asset](asset.svg)\n'
                '[web](https://example.test/absent#anchor)\n[mail](mailto:test@example.test)\n'
                '```md\n[example](missing.md)\n```\n', encoding="utf-8")
            self.assertEqual(check_docs.check_links([document], root), (4, []))

    def test_reports_missing_file_and_removed_heading_separately(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "guide.md").write_text("# Current\n", encoding="utf-8")
            document = root / "README.md"
            document.write_text("[old](guide.md#removed)\n[missing](gone.md)\n", encoding="utf-8")
            count, errors = check_docs.check_links([document], root)
            self.assertEqual(count, 2)
            self.assertEqual(errors, [
                "Broken heading anchor in README.md: guide.md#removed",
                "Broken link in README.md: gone.md",
            ])

    def test_links_cannot_read_outside_the_repository(self):
        with TemporaryDirectory() as directory:
            root = Path(directory).resolve() / "repo"
            root.mkdir()
            document = root / "README.md"
            document.write_text("[outside](../outside.md#secret)\n", encoding="utf-8")
            count, errors = check_docs.check_links([document], root)
            self.assertEqual(count, 1)
            self.assertEqual(errors, ["Link leaves repository in README.md: ../outside.md#secret"])


if __name__ == "__main__":
    unittest.main()
