"""Deterministic changelogs and the pre-commit hook against real temporary Git repos."""
from contextlib import redirect_stdout, redirect_stderr
from io import StringIO
import os
import shutil
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import check_docs
import update_changelog as changelog


class ChangelogTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory(prefix="changelog-test-")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        self.git("init", "-q", "-b", "master")
        self.git("config", "user.name", "Changelog Test")
        self.git("config", "user.email", "changelog@example.test")
        self.git("config", "commit.gpgSign", "false")
        (self.repo / "CHANGELOG.md").write_text("# 更新日志\n\n人工摘要。\n", encoding="utf-8")
        (self.repo / "source.txt").write_text("initial\n")
        self.first = self.commit("初始版本", "保留完整正文。\n第二行说明。")

    def git(self, *args, root=None):
        return subprocess.check_output(
            ["git", "-C", str(root or self.repo), *args], text=True,
            stderr=subprocess.PIPE,
        ).strip()

    def commit(self, title, body=None, root=None):
        root = root or self.repo
        self.git("add", "--all", root=root)
        args = ["commit", "--allow-empty", "-qm", title]
        if body:
            args.extend(["-m", body])
        self.git(*args, root=root)
        return self.git("rev-parse", "HEAD", root=root)

    def install_hook(self, root=None):
        """Use the repository's own hook and generator, as core.hooksPath does."""
        root = root or self.repo
        for name in (".githooks/pre-commit", "scripts/update_changelog.py", "scripts/check_docs.py"):
            (root / name).parent.mkdir(exist_ok=True)
            shutil.copy2(ROOT / name, root / name)
        self.git("config", "core.hooksPath", ".githooks", root=root)

    def run_hook(self, **env):
        clean = {key: value for key, value in os.environ.items()
                 if not key.startswith("GIT_") and key != changelog.SKIP_ENV}
        with patch.dict(os.environ, {**clean, **env}, clear=True), redirect_stdout(StringIO()), redirect_stderr(StringIO()):
            return changelog.hook(self.repo)

    def committed_paths(self, sha):
        return self.git("diff-tree", "--root", "--no-commit-id", "--name-only", "-r", sha).splitlines()

    def test_complete_history_includes_merged_empty_and_documentation_commits(self):
        self.git("checkout", "-qb", "topic")
        (self.repo / "topic.txt").write_text("topic\n")
        topic = self.commit("重复标题", "分支内的详细说明。")
        self.git("checkout", "-q", "master")
        (self.repo / "CHANGELOG.md").write_text("# 更新日志\n\n新的人工摘要。\n")
        docs = self.commit("重复标题")
        self.git("merge", "--no-ff", "-qm", "合并主题分支", "topic")
        merge = self.git("rev-parse", "HEAD")
        empty = self.commit("无文件差异的说明")
        history = changelog.commits(self.repo)
        self.assertEqual({entry.sha for entry in history}, {self.first, topic, docs, merge, empty})
        self.assertEqual(history[0].sha, empty)
        text = changelog.render("# 人工摘要\n", history)
        for sha in [self.first, topic, docs, merge, empty]:
            self.assertEqual(text.count(changelog.COMMIT_URL + sha), 1)
        self.assertIn("分支内的详细说明。", text)
        self.assertIn("第二行说明。", text)

    def test_managed_region_preserves_manual_text_and_is_idempotent(self):
        original = "# 人工摘要\r\n\r\n" + changelog.BEGIN + "\n旧记录\n" + changelog.END + "\r\n\r\n末尾说明。\r\n"
        result = changelog.render(original, changelog.commits(self.repo))
        self.assertTrue(result.startswith("# 人工摘要\r\n\r\n"))
        self.assertTrue(result.endswith("\r\n\r\n末尾说明。\r\n"))
        self.assertNotIn("旧记录", result)
        self.assertEqual(changelog.render(result, changelog.commits(self.repo)), result)

    def test_message_markup_fences_and_marker_text_stay_literal(self):
        body = "```\n" + changelog.BEGIN + "\n" + changelog.END + "\n```\n[private](../outside.md)\n</details><script>alert(1)</script>"
        entry = changelog.Commit("a" * 40, "2026-01-02", "[link](missing.md) <script> *text* `code`", body)
        text = changelog.render("# 摘要\n", [entry])
        self.assertIn("````text\n" + body + "\n````", text)
        self.assertIn("&#91;link&#93;", text)
        self.assertEqual(changelog.render(text, [entry]), text)
        path = self.base / "render.md"
        path.write_text(text)
        self.assertEqual(check_docs.check_links([path], self.base), (0, []))

    def test_malformed_markers_fail_without_overwriting_log(self):
        path = self.repo / "CHANGELOG.md"
        for text in [changelog.BEGIN, changelog.END, changelog.END + "\n" + changelog.BEGIN,
                     changelog.BEGIN + "\n" + changelog.BEGIN + "\n" + changelog.END]:
            with self.subTest(text=text):
                path.write_text(text)
                with self.assertRaisesRegex(ValueError, "markers"):
                    changelog.update(self.repo)
                self.assertEqual(path.read_text(), text)

    def test_only_marked_log_only_bot_commits_are_excluded(self):
        (self.repo / "source.txt").write_text("actual source change\n")
        source = self.commit("带同名尾注的功能变更", changelog.TRAILER)
        (self.repo / "CHANGELOG.md").write_text("# 手工更新\n")
        manual = self.commit("docs: 自动同步更新日志")
        with redirect_stdout(StringIO()):
            changelog.update(self.repo)
        bot = self.commit("docs: 自动同步更新日志", changelog.TRAILER)
        found = {entry.sha for entry in changelog.commits(self.repo)}
        self.assertIn(source, found)
        self.assertIn(manual, found)
        self.assertNotIn(bot, found)
        with redirect_stdout(StringIO()):
            self.assertFalse(changelog.update(self.repo, check=True))

    def test_preview_and_stale_check_do_not_change_working_log_or_index(self):
        path = self.repo / "CHANGELOG.md"
        before = path.read_bytes()
        index = self.git("write-tree")
        with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
            self.assertEqual(changelog.main(["check", "--root", str(self.repo)]), 1)
            preview = self.base / "preview.md"
            changelog.update(self.repo, preview)
        self.assertIn(self.first, preview.read_text())
        self.assertEqual(path.read_bytes(), before)
        self.assertEqual(self.git("write-tree"), index)

    def test_shallow_history_is_rejected_instead_of_silently_truncated(self):
        self.commit("第二条提交")
        shallow = self.base / "shallow"
        self.git("clone", "--quiet", "--depth=1", self.repo.as_uri(), str(shallow))
        with self.assertRaisesRegex(ValueError, "complete Git history"):
            changelog.commits(shallow)

    def test_atomic_write_preserves_concurrent_edits_and_rejects_symlinks(self):
        path = self.repo / "CHANGELOG.md"
        original = path.read_text()
        path.write_text(original + "用户并发补充。\n")
        with self.assertRaisesRegex(ValueError, "changed during generation"):
            changelog.write_log(path, "generated\n", expected=original)
        self.assertIn("用户并发补充。", path.read_text())
        link = self.base / "link.md"
        link.symlink_to(path)
        with self.assertRaisesRegex(ValueError, "non-regular"):
            changelog.write_log(link, "replacement\n")
        self.assertIn("用户并发补充。", path.read_text())
        self.assertFalse(list(self.repo.glob(".changelog-*")))

    def test_empty_repository_generates_an_explicit_empty_log(self):
        empty = self.base / "empty"
        empty.mkdir()
        self.git("init", "-q", "-b", "master", root=empty)
        self.assertEqual(changelog.commits(empty), [])
        self.assertIn("尚无提交记录", changelog.render("# 更新日志\n", []))

    def test_hook_commits_log_of_parent_history_without_extra_commits(self):
        self.install_hook()
        second = self.commit("启用提交前钩子")
        self.assertIn("CHANGELOG.md", self.committed_paths(second))
        text = self.git("show", f"{second}:CHANGELOG.md")
        self.assertIn(changelog.COMMIT_URL + self.first, text)
        self.assertNotIn(second, text)
        (self.repo / "source.txt").write_text("third\n")
        (self.repo / "untracked.txt").write_text("not staged by the hook\n")
        self.git("add", "source.txt")
        self.git("commit", "-qm", "第三次提交")
        third = self.git("rev-parse", "HEAD")
        self.assertEqual(sorted(self.committed_paths(third)), ["CHANGELOG.md", "source.txt"])
        text = self.git("show", f"{third}:CHANGELOG.md")
        for sha in (self.first, second):
            self.assertEqual(text.count(changelog.COMMIT_URL + sha), 1)
        self.assertNotIn(third, text)
        self.assertEqual(self.git("rev-list", "--count", "HEAD"), "3")
        self.assertEqual(self.git("status", "--porcelain"), "?? untracked.txt")

    def test_hook_skips_initial_commit_and_explicit_opt_out(self):
        empty = self.base / "empty"
        empty.mkdir()
        self.git("init", "-q", "-b", "master", root=empty)
        self.git("config", "user.name", "Changelog Test", root=empty)
        self.git("config", "user.email", "changelog@example.test", root=empty)
        self.install_hook(empty)
        (empty / "CHANGELOG.md").write_text("# 更新日志\n")
        initial = self.commit("初始提交", root=empty)
        self.assertEqual(self.git("show", f"{initial}:CHANGELOG.md", root=empty), "# 更新日志")
        before = (self.repo / "CHANGELOG.md").read_bytes()
        self.assertFalse(self.run_hook(**{changelog.SKIP_ENV: "1"}))
        self.assertEqual((self.repo / "CHANGELOG.md").read_bytes(), before)
        self.install_hook()
        self.git("add", "--all")
        subprocess.run(["git", "-C", str(self.repo), "commit", "-qm", "跳过日志"], check=True,
                       env={**os.environ, changelog.SKIP_ENV: "1"}, capture_output=True)
        self.assertNotIn("CHANGELOG.md", self.committed_paths(self.git("rev-parse", "HEAD")))

    def test_hook_keeps_unstaged_manual_edits_out_of_the_commit(self):
        self.assertTrue(self.run_hook())
        self.commit("记录日志")
        path = self.repo / "CHANGELOG.md"
        path.write_text(path.read_text().replace("人工摘要。", "人工摘要。\n\n未暂存的补充。"))
        (self.repo / "source.txt").write_text("staged\n")
        self.git("add", "source.txt")
        self.assertTrue(self.run_hook())
        staged = self.git("show", ":CHANGELOG.md")
        self.assertIn(self.git("rev-parse", "HEAD"), staged)
        self.assertNotIn("未暂存的补充。", staged)
        self.assertIn("未暂存的补充。", path.read_text())
        self.assertEqual(sorted(self.git("diff", "--cached", "--name-only").splitlines()), ["CHANGELOG.md", "source.txt"])
        unstaged = self.git("diff", "--", "CHANGELOG.md")
        self.assertEqual([line for line in unstaged.splitlines() if line[:1] in "+-" and line[:3] not in ("+++", "---")],
                         ["+未暂存的补充。", "+"])

    def test_hook_refuses_unstaged_edits_inside_generated_section(self):
        self.assertTrue(self.run_hook())
        self.commit("记录日志")
        path = self.repo / "CHANGELOG.md"
        edited = path.read_text().replace("## 逐次提交记录（自动生成）", "## 手改的生成区")
        path.write_text(edited)
        index = self.git("write-tree")
        with self.assertRaisesRegex(ValueError, "inside the generated commit log"):
            self.run_hook()
        self.assertEqual(path.read_text(), edited)
        self.assertEqual(self.git("write-tree"), index)

    def test_hook_skips_shallow_clones_and_path_commits_without_blocking(self):
        self.commit("第二条提交")
        shallow = self.base / "shallow"
        self.git("clone", "--quiet", "--depth=1", self.repo.as_uri(), str(shallow))
        before = (shallow / "CHANGELOG.md").read_bytes()
        with patch.object(self, "repo", shallow):
            self.assertFalse(self.run_hook())
        self.assertEqual((shallow / "CHANGELOG.md").read_bytes(), before)
        self.install_hook()
        self.commit("启用钩子")
        (self.repo / "source.txt").write_text("only this path\n")
        self.git("commit", "-qm", "仅提交指定路径", "source.txt")
        self.assertEqual(self.committed_paths(self.git("rev-parse", "HEAD")), ["source.txt"])
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_amend_logs_parents_instead_of_the_replaced_commit(self):
        self.install_hook()
        second = self.commit("启用钩子")
        (self.repo / "source.txt").write_text("draft\n")
        third = self.commit("待修订")
        (self.repo / "source.txt").write_text("amended\n")
        self.git("add", "source.txt")
        self.git("commit", "-q", "--amend", "-m", "已修订")
        amended = self.git("rev-parse", "HEAD")
        text = self.git("show", f"{amended}:CHANGELOG.md")
        self.assertIn(changelog.COMMIT_URL + second, text)
        self.assertNotIn(third, text)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_verbatim_commit_examples_are_recorded_by_the_hook(self):
        body = "示例：\n \techo ok\n保留末尾空格。  \n<<<<<<< example\n=======\n>>>>>>> example\n"
        self.git("commit", "--allow-empty", "--cleanup=verbatim", "-qm", "记录原始代码示例", "-m", body)
        source = self.git("rev-parse", "HEAD")
        self.assertTrue(self.run_hook())
        text = self.git("show", ":CHANGELOG.md")
        self.assertIn(source, text)
        self.assertIn(body.rstrip("\n"), text)
        self.assertFalse(self.run_hook())


if __name__ == "__main__":
    unittest.main()
