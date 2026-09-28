"""The workflow creates release refs with its own token and propagates API rejection."""
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import release_version


class ReleaseRefTests(unittest.TestCase):
    def test_workflow_posts_exact_ref_and_commit_without_token_in_arguments(self):
        environment = {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": "realmx/MantaSH",
                       "GH_TOKEN": "isolated-test-token"}
        with patch.dict(os.environ, environment, clear=True), patch.object(
            release_version.subprocess, "run"
        ) as request, patch.object(release_version, "git") as git:
            release_version.create_tag("v1.0.1", "a" * 40)
        request.assert_called_once()
        args, kwargs = request.call_args
        self.assertEqual(args[0], ["gh", "api", "--include", "--method", "POST",
                                   "repos/realmx/MantaSH/git/refs", "--input", "-"])
        self.assertEqual(json.loads(kwargs["input"]), {"ref": "refs/tags/v1.0.1", "sha": "a" * 40})
        self.assertTrue(kwargs["check"])
        self.assertNotIn(environment["GH_TOKEN"], str(request.call_args))
        git.assert_called_once_with("tag", "v1.0.1", "a" * 40)

    def test_api_rejection_does_not_create_local_tag_or_fall_back_to_push(self):
        environment = {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": "realmx/MantaSH",
                       "GH_TOKEN": "isolated-test-token"}
        with patch.dict(os.environ, environment, clear=True), patch.object(
            release_version.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["gh", "api"])
        ) as request, patch.object(release_version, "git") as git:
            with self.assertRaises(subprocess.CalledProcessError):
                release_version.create_tag("v1.0.1", "a" * 40)
        request.assert_called_once()
        git.assert_not_called()

    def test_workflow_requires_explicit_repository_and_token(self):
        for environment in ({"GITHUB_ACTIONS": "true"},
                            {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": "realmx/MantaSH"}):
            with self.subTest(environment=environment), patch.dict(os.environ, environment, clear=True), patch.object(
                release_version.subprocess, "run"
            ) as request:
                with self.assertRaises(ValueError):
                    release_version.create_tag("v1.0.1", "a" * 40)
                request.assert_not_called()
