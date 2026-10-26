#!/usr/bin/env python3
"""Check native file actions against owned files on the isolated loopback SFTP server.

Uses production row/confirmation handlers and real backend receipts. OS file pickers
and physical pointer input are separate acceptance boundaries.
"""
import argparse
import json
from pathlib import Path
import uuid

from qa_file_rows_macos import FileRowsAcceptance


class FileActionsAcceptance(FileRowsAcceptance):
    def direct(self, name, verb, **overrides):
        pane = self.pane(self.state())
        target = dict(session=pane["owner"], attempt=pane["attempt"],
                      request=pane["file_request"], path=str(Path(pane["file_path"]) / name),
                      verb=verb)
        target.update(overrides)
        return self.action("file_direct", **target)

    def exercise(self):
        self.report = self.root / "file-actions-results.json"
        self.results = {"checks": [], "scope": __doc__}
        profile = json.loads((self.fixture / "profile.json").read_text())["profile"]
        assert profile["host"] in ["127.0.0.1", "::1", "localhost"]
        remote = Path(json.loads((self.fixture / "fixture.json").read_text())["root"]).resolve() / ("actions-" + uuid.uuid4().hex)
        (remote / "destination").mkdir(parents=True)
        for name in ["a.txt", "b.txt", "c.txt"]:
            (remote / name).write_text("Owned file: " + name + "\n")
        self.wait(lambda s: len(s["tabs"]) == 1, "Fresh isolated workbench")
        self.action("profile", profile=profile)
        self.action("submit_profile", connect=True)
        self.wait(lambda s: s["modal"] == "trust", "Host identity checked before connection")
        self.action("trust")
        self.authenticate_fixture()
        self.wait(lambda s: self.pane(s)["state"] == "Connected", "Real SSH connected")
        self.directory(remote)
        self.row("b.txt")
        self.row("c.txt", additive=True)
        self.direct("a.txt", "copy")
        self.check(self.selected() == ["b.txt", "c.txt"], "Copy row button does not change multiselection")
        self.check(self.state()["file_operations"]["clipboard"] == [str(remote / "a.txt")], "Copy captures only its own row")
        self.direct("a.txt", "preview")
        self.check(self.selected() == ["b.txt", "c.txt"], "Preview row button does not change multiselection")
        self.wait(lambda s: s["modal"] == "editor" and s.get("editor", {}).get("preview") is True, "Preview opens read-only editor")
        self.action("keystroke", key="escape")
        old_request = self.pane(self.state())["file_request"]
        self.directory(remote / "destination")
        self.action("paste_files")
        self.wait(lambda s: s["file_operations"]["pending"] == 0 and "a.txt" in self.pane(s)["files"], "Clipboard pastes across directories using the real SFTP backend")
        self.check((remote / "a.txt").read_bytes() == (remote / "destination/a.txt").read_bytes(), "Copied bytes equal source bytes")
        self.action("paste_files")
        self.wait(lambda s: s["file_operations"]["pending"] == 0 and self.pane(s)["file_error"], "Copy collision returns a real failure")
        self.check((remote / "destination/a.txt").read_bytes() == (remote / "a.txt").read_bytes(), "Copy collision preserves destination bytes")
        self.directory(remote)
        self.direct("a.txt", "delete", request=old_request)
        self.check(self.state()["modal"] is None, "Old listing cannot invoke a row action")
        self.direct("a.txt", "delete", attempt=str(uuid.uuid4()))
        self.check(self.state()["modal"] is None, "Old attempt cannot invoke a row action")
        self.action("open", path=str(remote / "a.txt"))
        self.wait(lambda s: s["modal"] == "editor", "Real file opens for editing")
        self.action("editor_insert", text="draft ")
        self.wait(lambda s: s["editor"]["dirty"], "Unsaved draft established")
        self.action("keystroke", key="escape")
        self.row("b.txt")
        self.row("c.txt", additive=True)
        self.direct("a.txt", "rename")
        self.check(self.selected() == ["b.txt", "c.txt"], "Rename row button does not change multiselection")
        self.action("file_name_input", value="b.txt")
        self.action("confirm_file_operation")
        self.wait(lambda s: not s["commit"]["busy"] and s["commit"]["error"], "Rename collision keeps confirmation open with an error")
        self.check((remote / "a.txt").exists() and (remote / "b.txt").read_text() == "Owned file: b.txt\n", "Failed rename preserves both files")
        self.action("file_name_input", value="renamed.txt")
        self.action("confirm_file_operation")
        self.wait(lambda s: s["modal"] is None and "renamed.txt" in self.pane(s)["files"] and not self.pane(s)["file_loading"], "Failed rename can be corrected and retried")
        docs = self.pane(self.state())["documents"]
        self.check(len(docs) == 1 and docs[0]["path"] == str(remote / "renamed.txt") and docs[0]["dirty"], "Successful rename updates the unsaved draft path without losing edits")
        self.check(self.state()["file_operations"]["clipboard"] is None, "Rename invalidates stale clipboard paths")
        self.action("open", path=str(remote / "renamed.txt"))
        self.action("save")
        self.wait(lambda s: not s["editor"]["dirty"] and not self.pane(s)["documents"][0]["saving"], "Renamed draft saves through the new remote path")
        self.check((remote / "renamed.txt").read_text() == "draft Owned file: a.txt\n" and not (remote / "a.txt").exists(), "Save never recreates the old filename")
        self.action("keystroke", key="escape")
        self.direct("b.txt", "delete")
        frozen = self.state()["file_delete_targets"]
        self.directory(remote / "destination")
        self.action("confirm_file_operation")
        self.check((remote / "b.txt").exists() and self.state()["file_delete_targets"] == frozen and not self.state()["commit"]["busy"], "Directory change invalidates a frozen delete confirmation")
        self.action("dismiss")
        self.directory(remote)
        self.row("c.txt")
        self.direct("b.txt", "delete")
        self.check(self.selected() == ["c.txt"] and self.state()["file_delete_targets"]["paths"] == [str(remote / "b.txt")], "Delete row button captures only its own target without changing selection")
        self.action("confirm_file_operation")
        self.wait(lambda s: s["modal"] is None and not self.pane(s)["file_loading"] and "b.txt" not in self.pane(s)["files"], "Delete completes only after its real operation receipt")
        self.check(not (remote / "b.txt").exists() and (remote / "c.txt").exists(), "Confirmed deletion affects only the reviewed row")
        self.exercise_move(remote)
        print(json.dumps({"checks": len(self.results["checks"]), "report": str(self.report)}))

    def exercise_move(self, remote):
        (remote / "folder/inner").mkdir(parents=True)
        (remote / "folder/inner/deep.txt").write_text("Owned deep file\n")
        (remote / "destination/c.txt").write_text("Keep destination\n")
        self.directory(remote)
        self.direct("folder", "move")
        operations = self.state()["file_operations"]
        self.check(operations["clipboard"] == [str(remote / "folder")] and operations["clipboard_mode"] == "move", "Move row button marks a pending move")
        self.action("paste_files")
        self.wait(lambda s: s["file_operations"]["pending"] == 0 and self.pane(s)["file_error"], "Move into its own directory is refused visibly")
        self.directory(remote / "folder/inner")
        self.action("paste_files")
        self.wait(lambda s: s["file_operations"]["pending"] == 0 and self.pane(s)["file_error"], "Folder cannot move into its own subfolder")
        self.check((remote / "folder/inner/deep.txt").exists() and self.state()["file_operations"]["clipboard_mode"] == "move", "Refused moves change nothing and keep the pending move")
        self.directory(remote / "destination")
        self.action("paste_files")
        self.wait(lambda s: s["file_operations"]["pending"] == 0 and "folder" in self.pane(s)["files"] and not self.pane(s)["file_loading"], "Move lands through real SFTP rename")
        self.check((remote / "destination/folder/inner/deep.txt").read_text() == "Owned deep file\n" and not (remote / "folder").exists(), "Moved folder keeps its contents and leaves the source")
        self.check(self.state()["file_operations"]["clipboard"] is None and self.selected() == ["folder"], "Successful move clears the pending move and selects what landed")
        self.directory(remote)
        self.direct("c.txt", "move")
        self.directory(remote / "destination")
        self.action("paste_files")
        self.wait(lambda s: s["file_operations"]["pending"] == 0 and self.pane(s)["file_error"], "Move name conflict is refused")
        self.check((remote / "destination/c.txt").read_text() == "Keep destination\n" and (remote / "c.txt").exists(), "Move conflict never overwrites the existing target")
        self.action("cancel_file_clipboard")
        self.check(self.state()["file_operations"]["clipboard"] is None, "Pending move can be cancelled")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    args = parser.parse_args()
    FileActionsAcceptance(args.directory, args.fixture).exercise()


if __name__ == "__main__":
    main()
