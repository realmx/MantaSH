#!/usr/bin/env python3
"""Verify row selection with a fresh hidden native window and the macOS loopback SFTP fixture.

File gestures invoke the production row handler with explicit session/attempt/listing IDs.
Keyboard checks use GPUI dispatch. Browser pointer checks separately cover physical click semantics.
No delete confirmation is submitted and no non-loopback server is contacted.
"""
import argparse
import json
from pathlib import Path, PurePosixPath
import uuid

from qa_accept_macos import Acceptance


class FileRowsAcceptance(Acceptance):
    """Keep file selection evidence separate from the complete native acceptance suite."""

    def __init__(self, directory, fixture):
        super().__init__(directory, fixture)
        self.report = directory / "file-row-results.json"
        self.results = {"checks": [], "scope": __doc__}

    def draw(self):
        """Refresh a hidden window's native layout using a key with no file-list action."""
        self.action("draw")
        self.action("snapshot")

    def row(self, name, shift=False, additive=False, double=False, **overrides):
        """Pass the same immutable target fields captured by the real mouse-down handler."""
        pane = self.pane(self.state())
        values = {"session": pane["owner"], "attempt": pane["attempt"],
                  "request": pane["file_request"],
                  "path": str(PurePosixPath(pane["file_path"]) / name),
                  "shift": shift, "additive": additive, "double": double}
        values.update(overrides)
        self.action("file_row", **values)
        self.draw()

    def selected(self):
        """Compare real selected paths independently from HashSet iteration order."""
        return sorted(PurePosixPath(path).name for path in self.pane(self.state())["file_selection"])

    def directory(self, path):
        """Wait for the requested SFTP listing, not just request acknowledgement."""
        self.action("navigate", path=str(path))
        self.wait(lambda s: self.pane(s)["file_path"] == str(path) and
                  not self.pane(s)["file_loading"], "Loaded directory: " + path.name)
        self.draw()

    def exercise(self):
        """Exercise selection, navigation, visibility, keyboard and frozen batch targets."""
        profile = json.loads((self.fixture / "profile.json").read_text())["profile"]
        if profile["host"] not in ["127.0.0.1", "::1", "localhost"]:
            raise AssertionError("Only the dedicated loopback fixture is allowed")
        remote = Path(json.loads((self.fixture / "fixture.json").read_text())["root"]).resolve() / "row-selection"
        (remote / "folder").mkdir(parents=True, exist_ok=True)
        for name in ["a.txt", "b.txt", "c.txt", "d.txt", ".hidden.txt"]:
            (remote / name).write_text("Selection fixture: " + name + "\n")
        (remote / "folder/inside.txt").write_text("Nested fixture\n")
        self.wait(lambda s: len(s["tabs"]) == 1, "Fresh native workbench")
        self.action("profile", profile=profile)
        self.action("submit_profile", connect=True)
        self.wait(lambda s: s["modal"] == "trust", "Dedicated SSH host awaits verification")
        self.action("trust")
        self.wait(lambda s: self.pane(s)["state"] == "Connected", "Loopback SSH connected")
        self.action("tool", tool="files")
        self.directory(remote)
        empty_bounds = self.pane(self.state())["file_list_bounds"]
        self.row("b.txt")
        self.check(self.selected() == ["b.txt"], "Plain row click selects exactly one file")
        self.check(self.pane(self.state())["file_list_bounds"] == empty_bounds,
                   "First selection does not move or resize the file list")
        self.row("b.txt")
        self.check(self.selected() == ["b.txt"], "Repeated plain click keeps the row selected")
        self.row("d.txt", shift=True)
        self.check(self.selected() == ["b.txt", "c.txt", "d.txt"], "Forward inclusive Shift range")
        self.row("a.txt", shift=True)
        self.check(self.selected() == ["a.txt", "b.txt"], "Reverse Shift range retains its original pivot")
        self.row("d.txt", additive=True)
        self.row("a.txt", additive=True)
        self.check(self.selected() == ["b.txt", "d.txt"], "Additive click adds and removes an individual row")
        self.row("folder")
        self.check(self.pane(self.state())["file_path"] == str(remote), "Single folder click does not navigate")
        self.row("folder", double=True)
        self.wait(lambda s: self.pane(s)["file_path"] == str(remote / "folder") and
                  self.pane(s)["files"] == ["inside.txt"], "Double folder click reads the actual SFTP child directory")
        self.check(self.selected() == [] and self.pane(self.state())["file_anchor"] is None,
                   "Directory changes reset selection and pivot")
        self.row("inside.txt", shift=True)
        self.check(self.selected() == ["inside.txt"], "Shift without a pivot starts with the clicked file")
        self.directory(remote)
        self.action("toggle_hidden_files")
        self.row(".hidden.txt")
        self.action("toggle_hidden_files")
        self.draw()
        self.check(self.selected() == [] and self.pane(self.state())["file_anchor"] is None,
                   "Hiding selected files also removes their batch targets and pivot")
        self.row("b.txt")
        self.action("keystroke", key="cmd-a")
        self.draw()
        self.check(self.selected() == ["a.txt", "b.txt", "c.txt", "d.txt", "folder"],
                   "Cmd+A selects displayed entries only")
        self.action("keystroke", key="escape")
        self.draw()
        self.check(self.selected() == [], "Escape clears file selection")
        self.action("keystroke", key="down")
        self.draw()
        self.check(self.selected() == ["folder"], "Arrow navigation starts at the first displayed row")
        self.action("keystroke", key="shift-down")
        self.draw()
        self.check(self.selected() == ["a.txt", "folder"], "Keyboard Shift range remains available")
        original_request = self.pane(self.state())["file_request"]
        self.directory(remote / "folder")
        self.directory(remote)
        self.row("a.txt", request=original_request)
        self.check(self.selected() == [], "A stale listing cannot change the new selection")
        self.row("a.txt", attempt=str(uuid.uuid4()))
        self.check(self.selected() == [], "A stale connection attempt cannot change selection")
        self.row("b.txt")
        pane = self.pane(self.state())
        self.action("tab", index=0)
        self.action("file_row", session=pane["owner"], attempt=pane["attempt"],
                    request=pane["file_request"], path=str(remote / "d.txt"), shift=True)
        self.action("tab", index=1)
        self.check(self.selected() == ["b.txt"], "Detached rows from an inactive pane are ignored")
        self.row("d.txt", shift=True)
        self.action("review_file_delete")
        expected = self.state()["file_delete_targets"]
        self.check(expected["paths"] == [str(remote / name) for name in ["b.txt", "c.txt", "d.txt"]],
                   "Batch confirmation captures selected full paths in display order")
        self.action("tab", index=0)
        self.check(self.state()["file_delete_targets"] == expected,
                   "Switching tabs cannot retarget the batch confirmation")
        self.action("dismiss")
        self.action("tab", index=1)
        self.row("a.txt", double=True)
        self.wait(lambda s: any(d["path"] == str(remote / "a.txt") for d in self.pane(s)["documents"]),
                  "Double text-file click still opens the real remote document")
        self.check(all((remote / name).exists() for name in ["a.txt", "b.txt", "c.txt", "d.txt"]),
                   "Batch review did not delete any fixture files")
        print(json.dumps({"checks": len(self.results["checks"]), "report": str(self.report)}))


def main():
    """Use only the explicitly named native window and loopback test fixture."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    args = parser.parse_args()
    FileRowsAcceptance(args.directory, args.fixture).exercise()


if __name__ == "__main__":
    main()
