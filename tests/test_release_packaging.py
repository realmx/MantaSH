"""Release selection, version allocation and native packaging policy."""
import csv
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import package_release
import release_matrix
import release_notes
import release_version
import update_homebrew_cask

SHA = "f" * 40


class ReleaseMatrixTests(unittest.TestCase):
    def test_tag_push_and_manual_tag_publish_all_targets(self):
        for event, platform in (("push", "all"), ("workflow_dispatch", "windows-x64")):
            with self.subTest(event=event):
                result = release_matrix.select("tag", "v1.1.1", event, platform, SHA, "1.1.0")
                self.assertEqual(result["version"], "1.1.1")
                self.assertEqual(result["package_version"], "1.1.1")
                self.assertEqual(result["publish"], "true")
                names = [row["name"] for row in json.loads(result["matrix"])["include"]]
                self.assertEqual(names, ["windows-x64", "windows-x86", "windows-arm64", "macos-arm64", "macos-x64"])
                self.assertTrue(all(row["os"].startswith(("windows-", "macos-")) for row in json.loads(result["matrix"])["include"]))

    def test_build_branches_and_manual_selection_only_upload_dev_artifacts(self):
        for ref, event, platform, expected in (
            ("build/all", "push", "all", 5),
            ("build/windows-x64", "push", "all", 1),
            ("build/macos-arm64", "push", "all", 1),
            ("master", "workflow_dispatch", "macos-arm64", 1),
            ("master", "workflow_dispatch", "all", 5),
        ):
            with self.subTest(ref=ref, event=event):
                result = release_matrix.select("branch", ref, event, platform, SHA, "1.0.10")
                self.assertEqual(result["package_version"], "1.0.10-dev.fffffff")
                self.assertEqual(result["publish"], "false")
                self.assertEqual(len(json.loads(result["matrix"])["include"]), expected)

    def test_invalid_ref_and_older_tag_fail_before_build(self):
        for ref_type, ref_name, event in (
            ("branch", "master", "push"),
            ("tag", "v1.0.9", "push"),
            ("tag", "trial-v1.1.1", "push"),
            ("branch", "build/", "pull_request"),
        ):
            with self.subTest(ref=ref_name), self.assertRaises(ValueError):
                release_matrix.select(ref_type, ref_name, event, "all", SHA, "1.0.10")

    def test_outputs_are_valid_for_github_matrix(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            env = {"GITHUB_OUTPUT": str(output), "GITHUB_REF_TYPE": "branch",
                   "GITHUB_REF_NAME": "build/windows-x64", "GITHUB_EVENT_NAME": "push",
                   "GITHUB_SHA": SHA, "INPUT_PLATFORM": "all"}
            with patch.dict(os.environ, env), redirect_stdout(StringIO()):
                release_matrix.main()
            self.assertIn("publish=false\n", output.read_text())
            self.assertIn('"name":"windows-x64"', output.read_text())


class ReleaseVersionTests(unittest.TestCase):
    def test_next_version_uses_tag_or_manual_baseline(self):
        self.assertEqual(release_version.next_version("1.0.0", []), "1.0.0")
        self.assertEqual(release_version.next_version("1.0.10", ["v1.0.7", "v1.0.10"]), "1.0.11")
        self.assertEqual(release_version.next_version("2.0.0", ["v1.0.10"]), "2.0.0")
        self.assertEqual(release_version.next_version("1.0.10", ["v2.0.0"]), "2.0.1")

    def test_documentation_scope_excludes_release_metadata_and_code(self):
        docs = ["README.md", "README.en.md", "AGENTS.md", "THIRD_PARTY.md",
                "docs/user-guide.md", "docs/nested/notes.md", "assets/screenshots/ssh-editor.png"]
        self.assertTrue(release_version.documentation_only(docs))
        self.assertFalse(release_version.documentation_only([]))
        for path in ("src/main.rs", "scripts/release_version.py", ".github/workflows/version.yml",
                     "Cargo.toml", "docs/dependency-licenses.csv", "examples/connections.csv",
                     "assets/icons/save.svg", "vendor/taffy-compat/README.md"):
            with self.subTest(path=path):
                self.assertFalse(release_version.documentation_only(docs + [path]))

    def test_changed_paths_includes_deleted_rename_source_and_unusual_filenames(self):
        with patch.object(release_version.subprocess, "check_output", return_value=b"src/old.rs\0docs/new.md\0docs/name\nline.md\0") as changed:
            self.assertEqual(release_version.changed_paths_since("v1.0.10", SHA),
                             ["src/old.rs", "docs/new.md", "docs/name\nline.md"])
        changed.assert_called_once_with(
            ["git", "diff", "--no-renames", "--name-only", "-z", "v1.0.10", SHA, "--"],
            cwd=ROOT,
        )

    def test_real_git_tree_diff_skips_docs_until_code_changes(self):
        with TemporaryDirectory() as directory:
            repo = Path(directory)
            def git(*args):
                return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()
            def commit(message):
                git("add", "--all")
                git("-c", "user.name=QA", "-c", "user.email=qa@example.test",
                    "commit", "-qm", message)
                return git("rev-parse", "HEAD")

            git("init", "-q")
            (repo / "src").mkdir()
            (repo / "src/main.rs").write_text("fn main() {}\n")
            (repo / "README.md").write_text("MantaSH\n")
            initial = commit("baseline")
            git("tag", "v1.0.0")
            (repo / "docs").mkdir()
            (repo / "docs/user-guide.md").write_text("User guide\n")
            (repo / "assets/screenshots").mkdir(parents=True)
            (repo / "assets/screenshots/local-workspace.png").write_bytes(b"screenshot")
            docs_commit = commit("documentation")
            self.assertFalse(release_version.release_relevant_changes("v1.0.0", docs_commit, repo))
            (repo / "README.md").write_text("Updated MantaSH\n")
            more_docs = commit("more documentation")
            self.assertFalse(release_version.release_relevant_changes("v1.0.0", more_docs, repo))
            (repo / "src/main.rs").rename(repo / "docs/moved.md")
            source_change = commit("move source into docs")
            paths = release_version.changed_paths_since("v1.0.0", source_change, repo)
            self.assertIn("src/main.rs", paths)
            self.assertIn("docs/moved.md", paths)
            self.assertTrue(release_version.release_relevant_changes("v1.0.0", source_change, repo))
            self.assertNotEqual(initial, source_change)
    def test_documentation_push_skips_tag_until_release_relevant_change(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            calls = []
            def git(*args):
                calls.append(args)
                if args == ("rev-parse", "HEAD"):
                    return SHA
                if args[:2] == ("tag", "--points-at"):
                    return ""
                if args == ("tag", "--merged", "origin/master"):
                    return "v1.1.5"
                if args == ("tag", "--list"):
                    return "v1.1.5"
                if args == ("tag", "v1.1.6", SHA):
                    return ""
                raise AssertionError(args)
            with patch.object(release_version, "git", side_effect=git), patch.object(
                release_version, "changed_paths_since"
            ) as changed, patch.object(release_version.subprocess, "run") as runner, patch.dict(
                os.environ, {"GITHUB_OUTPUT": str(output)}, clear=True
            ):
                runner.return_value.returncode = 0
                for files in (["docs/user-guide.md", "assets/screenshots/ssh-editor.png"],
                              ["README.md", "docs/release.md"]):
                    changed.return_value = files
                    self.assertEqual(release_version.main(["tag"]), 0)
                    self.assertNotIn(("tag", "v1.1.6", SHA), calls)
                self.assertEqual(runner.call_count, 2)
                changed.return_value = ["README.md", "src/ui/mod.rs"]
                self.assertEqual(release_version.main(["tag"]), 0)
            self.assertEqual(changed.call_count, 3)
            changed.assert_called_with("v1.1.5", SHA, ROOT)
            self.assertIn(("tag", "v1.1.6", SHA), calls)
            self.assertEqual(runner.call_count, 4)
            self.assertEqual(output.read_text(),
                             "version=1.1.5\ncreated=false\n" * 2 + "version=1.1.6\ncreated=true\n")

    def test_empty_tree_diff_does_not_create_a_tag(self):
        with patch.object(release_version, "changed_paths_since", return_value=[]):
            self.assertFalse(release_version.release_relevant_changes("v1.0.0", SHA))

    def test_repeat_does_not_create_another_tag(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            def git(*args):
                if args == ("rev-parse", "HEAD"):
                    return SHA
                if args[:2] == ("tag", "--points-at"):
                    return "v1.1.1"
                raise AssertionError(args)
            with patch.object(release_version, "git", side_effect=git), patch.object(
                release_version.subprocess, "run"
            ) as push, patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}, clear=True):
                self.assertEqual(release_version.main(["tag"]), 0)
            self.assertEqual(output.read_text(), "version=1.1.1\ncreated=false\n")
            push.assert_not_called()

    def test_new_push_tags_source_without_version_commit(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            calls = []
            def git(*args):
                calls.append(args)
                if args == ("rev-parse", "HEAD"):
                    return SHA
                if args[:2] == ("tag", "--points-at"):
                    return ""
                if args == ("tag", "--merged", "origin/master"):
                    return "v1.0.7\nv1.1.5"
                if args == ("tag", "--list"):
                    return "v1.0.7\nv1.1.5"
                if args == ("tag", "v1.1.6", SHA):
                    return ""
                raise AssertionError(args)
            with patch.object(release_version, "git", side_effect=git), patch.object(
                release_version, "changed_paths_since", return_value=["src/main.rs", "README.md"]
            ), patch.object(release_version.subprocess, "run"
            ) as push, patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}, clear=True):
                push.return_value.returncode = 0
                self.assertEqual(release_version.main(["tag"]), 0)
            self.assertEqual(output.read_text(), "version=1.1.6\ncreated=true\n")
            self.assertTrue(all(args[0] != "commit" for args in calls))
            self.assertEqual(push.call_args.args[0], ["git", "push", "--porcelain", "origin", f"{SHA}:refs/tags/v1.1.6"])

    def test_old_push_is_not_tagged_after_newer_source(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            def git(*args):
                if args == ("rev-parse", "HEAD"):
                    return "e" * 40
                if args[:2] == ("tag", "--points-at"):
                    return ""
                if args == ("tag", "--merged", "origin/master"):
                    return "v1.1.1"
                raise AssertionError(args)
            with patch.object(release_version, "git", side_effect=git), patch.object(
                release_version.subprocess, "run"
            ) as runner, patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}, clear=True):
                runner.return_value.returncode = 1
                self.assertEqual(release_version.main(["tag"]), 0)
            self.assertEqual(output.read_text(), "version=1.1.1\ncreated=false\n")
            self.assertEqual(runner.call_count, 1)

    def test_rewritten_master_keeps_version_above_unmerged_release_tag(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            def git(*args):
                if args == ("rev-parse", "HEAD"):
                    return SHA
                if args[:2] == ("tag", "--points-at"):
                    return ""
                if args == ("tag", "--merged", "origin/master"):
                    return ""
                if args == ("tag", "--list"):
                    return "v1.1.5\ntrial-v9.0.0"
                if args == ("tag", "v1.1.6", SHA):
                    return ""
                raise AssertionError(args)
            with patch.object(release_version, "git", side_effect=git), patch.object(
                release_version.subprocess,
                "run",
                return_value=subprocess.CompletedProcess(["git", "push"], 0, "", ""),
            ) as push, patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}, clear=True):
                self.assertEqual(release_version.main(["tag"]), 0)
            self.assertEqual(output.read_text(), "version=1.1.6\ncreated=true\n")
            self.assertEqual(push.call_args.args[0][-1], f"{SHA}:refs/tags/v1.1.6")
    def test_stage_versions_metadata_only_in_runner_copy(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            paths = ("Cargo.toml", "Cargo.lock", "docs/dependency-licenses.csv")
            for name in paths:
                shutil.copy2(ROOT / name, root / name)
            baseline = release_version.source_version(root)
            major, minor, patch_number = release_version.parts(baseline)
            version = f"{major}.{minor}.{patch_number + 1}"
            release_version.stage(root, version)
            self.assertEqual(release_version.source_version(root), version)
            self.assertIn(f'name = "mantash"\nversion = "{version}"', (root / "Cargo.lock").read_text())
            with (root / "docs/dependency-licenses.csv").open(newline="") as stream:
                records = list(csv.DictReader(stream))
            self.assertEqual([row["version"] for row in records if row["package"] == "mantash"], [version])
            self.assertEqual(release_version.source_version(ROOT), baseline)
            with patch.dict(os.environ, {"GITHUB_ACTIONS": "true", "GITHUB_REF_TYPE": "tag",
                                      "GITHUB_REF_NAME": "v" + version}, clear=True), redirect_stdout(StringIO()):
                self.assertEqual(release_version.main(["validate", "--version", version, "--root", str(root)]), 0)
            with patch.dict(os.environ, {"GITHUB_ACTIONS": "true", "GITHUB_REF_TYPE": "branch",
                                      "GITHUB_REF_NAME": "master"}, clear=True), redirect_stderr(StringIO()):
                self.assertEqual(release_version.main(["validate", "--version", version, "--root", str(root)]), 1)

    def test_invalid_metadata_does_not_partially_stage(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            for name in ("Cargo.toml", "Cargo.lock"):
                shutil.copy2(ROOT / name, root / name)
            (root / "docs/dependency-licenses.csv").write_text("package,version\nmantash,broken\n")
            before = (root / "Cargo.toml").read_bytes()
            with self.assertRaisesRegex(ValueError, "license metadata"):
                release_version.stage(root, "99.0.0")
            self.assertEqual((root / "Cargo.toml").read_bytes(), before)


class ReleasePackageTests(unittest.TestCase):
    def test_notes_disclose_unnotarized_and_unsigned_packages(self):
        with TemporaryDirectory() as directory:
            output = Path(directory) / "release.md"
            argv = ["release_notes.py", "--version", "1.2.3", "--output", str(output)]
            with patch.object(sys, "argv", argv), patch.object(release_notes, "git", return_value="commit"), patch.object(
                release_notes, "previous_tag", return_value=None
            ), patch.object(release_notes, "subjects", return_value=["Fix connection handling"]):
                self.assertEqual(release_notes.main(), 0)
            body = output.read_text()
            self.assertIn("ad-hoc signed and not Apple notarized", body)
            self.assertIn("Windows x86, x64 and arm64 unsigned installers", body)
            self.assertIn("Fix connection handling", body)

    def test_macos_bundle_uses_ad_hoc_signature(self):
        with TemporaryDirectory() as directory:
            binary = Path(directory) / "mantash"
            binary.write_bytes(b"test binary")
            with patch.object(package_release.subprocess, "run") as run:
                app = package_release.macos_app(Path(directory) / "stage", binary, "1.2.3", "aarch64-apple-darwin")
            info = json.loads((app / "Contents/Resources/build-info.json").read_text())
            self.assertEqual(info["version"], "1.2.3")
            self.assertIn("not Developer ID signed or Apple notarized", info["signing"])
            self.assertIn(["/usr/bin/codesign", "--force", "--deep", "--sign", "-", str(app)],
                          [call.args[0] for call in run.call_args_list])

    def test_macos_dmg_has_matching_checksum_and_no_zip(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "mantash"
            binary.write_bytes(b"test binary")
            def native(command, **_):
                if command[:2] == ["/usr/bin/hdiutil", "create"]:
                    Path(command[-1]).write_bytes(b"disk image")
            argv = ["package_release.py", "--target", "aarch64-apple-darwin",
                    "--binary", str(binary), "--version", "1.2.3", "--package-version", "1.2.3-dev.abcdef0",
                    "--output-dir", str(root / "dist")]
            with patch.object(package_release, "sys", SimpleNamespace(platform="darwin")), patch.object(
                sys, "argv", argv
            ), patch.object(package_release.subprocess, "run", side_effect=native), redirect_stdout(StringIO()):
                self.assertEqual(package_release.main(), 0)
            path = root / "dist/MantaSH-1.2.3-dev.abcdef0-macos-arm64.dmg"
            self.assertEqual(path.with_name(path.name + ".sha256").read_text(),
                             f"{package_release.digest(path)}  {path.name}\n")
            self.assertEqual(sorted(item.name for item in (root / "dist").iterdir()),
                             [path.name, path.name + ".sha256"])

    @staticmethod
    def windows_binary(magic=0x20B, subsystem=2):
        """Minimal PE headers for packaging tests; not an executable program."""
        optional_size = 224 if magic == 0x10B else 240
        data = bytearray(128 + 24 + optional_size)
        data[:2] = b"MZ"
        data[60:64] = (128).to_bytes(4, "little")
        data[128:132] = b"PE\0\0"
        data[148:150] = optional_size.to_bytes(2, "little")
        data[152:154] = magic.to_bytes(2, "little")
        data[220:222] = subsystem.to_bytes(2, "little")
        return bytes(data)

    def test_windows_console_binary_is_rejected_before_staging(self):
        for magic in (0x10B, 0x20B):
            with self.subTest(magic=magic), TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / "mantash.exe"
                binary.write_bytes(self.windows_binary(magic, subsystem=3))
                with self.assertRaisesRegex(ValueError, "GUI subsystem 2, got 3"):
                    package_release.windows_tree(root / "stage", binary, "1.2.3", "x86_64-pc-windows-msvc")
                self.assertFalse((root / "stage").exists())

    def test_windows_gui_header_validation_rejects_malformed_input(self):
        invalid = [b"MZ", self.windows_binary()[:170], self.windows_binary(magic=0)]
        missing_signature = bytearray(self.windows_binary())
        missing_signature[128:132] = b"BAD!"
        invalid.append(bytes(missing_signature))
        for data in invalid:
            with self.subTest(length=len(data)), TemporaryDirectory() as directory:
                binary = Path(directory) / "mantash.exe"
                binary.write_bytes(data)
                with self.assertRaises(ValueError):
                    package_release.validate_windows_gui(binary)

    def test_windows_icon_resource_is_required_and_module_is_released(self):
        for resource in (0, 456):
            with self.subTest(resource=resource), TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / "mantash.exe"
                binary.write_bytes(self.windows_binary())
                with patch.object(package_release, "os", SimpleNamespace(name="nt")), patch(
                    "ctypes.WinDLL", create=True
                ) as dll:
                    kernel = dll.return_value
                    kernel.LoadLibraryExW.return_value = 123
                    kernel.FindResourceW.return_value = resource
                    if resource:
                        package_release.validate_windows_icon(binary)
                    else:
                        with self.assertRaisesRegex(ValueError, "missing application icon resource 1"):
                            package_release.windows_tree(root / "stage", binary, "1.2.3", "x86_64-pc-windows-msvc")
                        self.assertFalse((root / "stage").exists())
                    kernel.FindResourceW.assert_called_once_with(123, 1, 14)
                    kernel.FreeLibrary.assert_called_once_with(123)

    def test_windows_installer_contains_staged_binary_and_has_checksum(self):
        for target, name, arch in (("i686-pc-windows-msvc", "x86", "x86compatible"),
                                   ("x86_64-pc-windows-msvc", "x64", "x64compatible"),
                                   ("aarch64-pc-windows-msvc", "arm64", "arm64")):
            with self.subTest(name=name), TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / "mantash.exe"
                binary.write_bytes(self.windows_binary(0x10B if name == "x86" else 0x20B))
                calls = []
                def compile_installer(command, **_):
                    calls.append(command)
                    directory_arg = next(item[12:] for item in command if item.startswith("/DOutputDir="))
                    basename = next(item[13:] for item in command if item.startswith("/DOutputBase="))
                    staged = Path(next(item[12:] for item in command if item.startswith("/DSourceDir=")))
                    self.assertEqual((staged / "mantash.exe").read_bytes(), binary.read_bytes())
                    info = json.loads((staged / "build-info.json").read_text())
                    self.assertEqual(info["version"], "1.2.3")
                    self.assertIn("Unsigned", info["signing"])
                    (Path(directory_arg) / f"{basename}.exe").write_bytes(b"MZ installer")
                argv = ["package_release.py", "--target", target, "--binary", str(binary),
                        "--version", "1.2.3", "--output-dir", str(root / "dist")]
                with patch.object(package_release, "sys", SimpleNamespace(platform="win32")), patch.object(
                    sys, "argv", argv
                ), patch.object(package_release.subprocess, "run", side_effect=compile_installer), patch.object(
                    package_release, "validate_windows_icon"
                ), redirect_stdout(StringIO()):
                    self.assertEqual(package_release.main(), 0)
                self.assertIn(f"/DBuildArch={arch}", calls[0])
                installer = root / "dist" / f"MantaSH-1.2.3-windows-{name}-setup.exe"
                self.assertEqual(installer.with_name(installer.name + ".sha256").read_text(),
                                 f"{package_release.digest(installer)}  {installer.name}\n")
                self.assertEqual(sorted(item.name for item in (root / "dist").iterdir()),
                                 [installer.name, installer.name + ".sha256"])

    def test_invalid_dev_name_rejected_before_package(self):
        with TemporaryDirectory() as directory:
            binary = Path(directory) / "mantash.exe"
            binary.write_bytes(b"MZ")
            argv = ["package_release.py", "--target", "x86_64-pc-windows-msvc", "--binary", str(binary),
                    "--version", "1.2.3", "--package-version", "9.9.9-dev.fffffff",
                    "--output-dir", str(Path(directory) / "dist")]
            with patch.object(sys, "argv", argv), redirect_stderr(StringIO()), self.assertRaises(SystemExit):
                package_release.main()
            self.assertFalse((Path(directory) / "dist").exists())

    def test_cask_uses_verified_macos_dmg_hashes(self):
        with TemporaryDirectory() as directory:
            assets = Path(directory)
            for arch in ("arm64", "x64"):
                path = assets / f"MantaSH-1.2.3-macos-{arch}.dmg"
                path.write_bytes(arch.encode())
                package_release.write_checksum(path)
            arm = update_homebrew_cask.checksum(assets, "1.2.3", "arm64")
            intel = update_homebrew_cask.checksum(assets, "1.2.3", "x64")
            body = update_homebrew_cask.render("1.2.3", arm, intel)
            self.assertIn(arm, body)
            self.assertIn(intel, body)
            self.assertIn('app "MantaSH.app"', body)
            (assets / "MantaSH-1.2.3-macos-arm64.dmg").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "Invalid release checksum"):
                update_homebrew_cask.checksum(assets, "1.2.3", "arm64")


if __name__ == "__main__":
    unittest.main()
