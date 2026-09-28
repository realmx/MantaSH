"""Release refs use Git transport and never overwrite an existing remote tag."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import release_version


class ReleaseRefTests(unittest.TestCase):
    def test_push_uses_exact_commit_without_force_or_credentials_in_arguments(self):
        with patch.object(release_version.subprocess, "run") as request, patch.object(
            release_version, "git"
        ) as git:
            request.return_value.returncode = 0
            request.return_value.stdout = ""
            request.return_value.stderr = ""
            release_version.create_tag("v1.0.1", "a" * 40)
        request.assert_called_once_with(
            ["git", "push", "--porcelain", "origin", f'{"a" * 40}:refs/tags/v1.0.1'],
            check=False,
            capture_output=True,
            text=True,
        )
        git.assert_called_once_with("tag", "v1.0.1", "a" * 40)

    def test_rejected_push_does_not_record_local_success_or_retry(self):
        result = subprocess.CompletedProcess(
            ["git", "push"], 1, "", "remote rejected"
        )
        with patch.object(release_version.subprocess, "run", return_value=result) as request, patch.object(
            release_version, "git"
        ) as git:
            with self.assertRaises(subprocess.CalledProcessError):
                release_version.create_tag("v1.0.1", "a" * 40)
        request.assert_called_once()
        git.assert_not_called()

    def test_reserved_tag_advances_to_next_patch_version(self):
        calls = []
        blocked = subprocess.CompletedProcess(
            ["git", "push"], 1, "", "GH013: Cannot create ref due to creations being restricted"
        )
        accepted = subprocess.CompletedProcess(["git", "push"], 0, "", "")

        def run(command, **kwargs):
            calls.append(command)
            return blocked if command[-1].endswith("v1.0.1") else accepted

        with patch.object(release_version.subprocess, "run", side_effect=run), patch.object(
            release_version, "git", return_value=""
        ) as git:
            self.assertEqual(
                release_version.create_next_tag("1.0.0", "a" * 40, ["v1.0.0"]),
                "1.0.2",
            )
        self.assertEqual(calls[0][-1], "a" * 40 + ":refs/tags/v1.0.1")
        self.assertEqual(calls[1][-1], "a" * 40 + ":refs/tags/v1.0.2")
        git.assert_called_once_with("tag", "v1.0.2", "a" * 40)

    def test_real_remote_tag_is_created_at_source_and_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            remote = root / "remote.git"
            repo = root / "source"
            subprocess.run(["git", "init", "--bare", str(remote)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["git", "init", str(repo)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

            def git(*args):
                return subprocess.check_output(["git", *args], cwd=repo, text=True).strip()

            git("config", "user.name", "Release Test")
            git("config", "user.email", "release-test@example.invalid")
            git("remote", "add", "origin", str(remote))
            git("commit", "--allow-empty", "-m", "source")
            first = git("rev-parse", "HEAD")
            git("commit", "--allow-empty", "-m", "newer source")
            second = git("rev-parse", "HEAD")
            previous = Path.cwd()
            try:
                os.chdir(repo)
                release_version.create_tag("v1.0.1", first)
                self.assertEqual(git("rev-parse", "refs/tags/v1.0.1"), first)
                self.assertEqual(git("ls-remote", "origin", "refs/tags/v1.0.1").split()[0], first)
                git("tag", "-d", "v1.0.1")
                with self.assertRaises(subprocess.CalledProcessError):
                    release_version.create_tag("v1.0.1", second)
                self.assertEqual(git("tag", "--list"), "")
                self.assertEqual(git("ls-remote", "origin", "refs/tags/v1.0.1").split()[0], first)
            finally:
                os.chdir(previous)
