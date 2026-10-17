"""Deterministic changelogs and safe publication against real temporary Git repos."""
from contextlib import redirect_stdout, redirect_stderr
from io import StringIO
import os
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
        # Publisher validation is exercised with a repository-owned successful checker.
        (self.repo / "scripts").mkdir()
        (self.repo / "scripts/check_docs.py").write_text("raise SystemExit(0)\n")
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

    def remote(self):
        remote = self.base / "remote.git"
        self.git("init", "--bare", "-q", str(remote))
        self.git("remote", "add", "origin", str(remote))
        self.git("push", "-q", "origin", "master")
        return remote

    def assert_no_extra_worktrees(self):
        listing = self.git("worktree", "list", "--porcelain")
        self.assertEqual(sum(line.startswith("worktree ") for line in listing.splitlines()), 1)

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

    def test_publish_is_idempotent_and_preserves_caller_worktree_and_index(self):
        remote = self.remote()
        (self.repo / "source.txt").write_text("staged user edit\n")
        self.git("add", "source.txt")
        (self.repo / "source.txt").write_text("unstaged user edit\n")
        before_status, before_index = self.git("status", "--porcelain"), self.git("write-tree")
        before_log = (self.repo / "CHANGELOG.md").read_bytes()
        with redirect_stdout(StringIO()):
            self.assertTrue(changelog.publish(self.repo))
            head = self.git("rev-parse", "master", root=remote)
            self.assertFalse(changelog.publish(self.repo))
        self.assertEqual(self.git("rev-parse", "master", root=remote), head)
        self.assertEqual(self.git("status", "--porcelain"), before_status)
        self.assertEqual(self.git("write-tree"), before_index)
        self.assertEqual((self.repo / "CHANGELOG.md").read_bytes(), before_log)
        self.assertEqual((self.repo / "source.txt").read_text(), "unstaged user edit\n")
        self.assertIn(self.first, self.git("show", "master:CHANGELOG.md", root=remote))
        self.assertEqual(self.git("diff-tree", "--no-commit-id", "--name-only", "-r", head, root=remote), "CHANGELOG.md")
        self.assert_no_extra_worktrees()

    def test_publish_regenerates_after_a_real_non_fast_forward_race(self):
        remote = self.remote()
        other = self.base / "other"
        self.git("clone", "--quiet", "--branch", "master", str(remote), str(other))
        self.git("config", "user.name", "Other", root=other)
        self.git("config", "user.email", "other@example.test", root=other)
        self.git("config", "commit.gpgSign", "false", root=other)
        original_git = changelog.git
        pushed = []
        concurrent = []

        def race(root, *args, **kwargs):
            if args[:1] == ("push",):
                pushed.append(args)
                if len(pushed) == 1:
                    (other / "concurrent.txt").write_text("keep this change\n")
                    concurrent.append(self.commit("并发更新", root=other))
                    self.git("push", "--quiet", "origin", "master", root=other)
            return original_git(root, *args, **kwargs)

        with patch.object(changelog, "git", side_effect=race), redirect_stdout(StringIO()):
            self.assertTrue(changelog.publish(self.repo))
        self.assertEqual(len(pushed), 2)
        self.assertTrue(all(args == ("push", "origin", "HEAD:refs/heads/master") for args in pushed))
        text = self.git("show", "master:CHANGELOG.md", root=remote)
        self.assertIn(self.first, text)
        self.assertIn(concurrent[0], text)
        self.assertEqual(self.git("show", "master:concurrent.txt", root=remote), "keep this change")
        self.assert_no_extra_worktrees()

    def test_rejected_push_preserves_remote_and_cleans_only_owned_checkout(self):
        remote = self.remote()
        hook = remote / "hooks/pre-receive"
        hook.write_text("#!/bin/sh\nprintf 'test policy rejection\\n' >&2\nexit 1\n")
        hook.chmod(0o755)
        before = self.git("rev-parse", "master", root=remote)
        with redirect_stdout(StringIO()), self.assertRaisesRegex(RuntimeError, "test policy rejection"):
            changelog.publish(self.repo)
        self.assertEqual(self.git("rev-parse", "master", root=remote), before)
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.assert_no_extra_worktrees()

    def test_failed_document_validation_cannot_publish(self):
        remote = self.remote()
        before = self.git("rev-parse", "master", root=remote)
        with patch.object(changelog, "validate_documents", side_effect=ValueError("broken docs")), redirect_stdout(StringIO()):
            with self.assertRaisesRegex(ValueError, "broken docs"):
                changelog.publish(self.repo)
        self.assertEqual(self.git("rev-parse", "master", root=remote), before)
        self.assert_no_extra_worktrees()

    def test_verbatim_commit_examples_do_not_block_publication(self):
        body = "示例：\n \techo ok\n保留末尾空格。  \n<<<<<<< example\n=======\n>>>>>>> example\n"
        self.git("commit", "--allow-empty", "--cleanup=verbatim", "-qm", "记录原始代码示例", "-m", body)
        source = self.git("rev-parse", "HEAD")
        remote = self.remote()
        with redirect_stdout(StringIO()):
            self.assertTrue(changelog.publish(self.repo))
        text = self.git("show", "master:CHANGELOG.md", root=remote)
        self.assertIn(source, text)
        self.assertIn(body.rstrip("\n"), text)
        self.assert_no_extra_worktrees()


if __name__ == "__main__":
    unittest.main()
