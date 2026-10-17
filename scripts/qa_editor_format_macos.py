#!/usr/bin/env python3
"""Exercise explicit formatting in the native editor using an isolated loopback file.

Uses production editing/formatting and GPUI keyboard dispatch, not physical keyboard
or OS input-method acceptance. Snapshots contain hashes and geometry, not file text.
"""
import argparse
import hashlib
import json
from pathlib import Path

from qa_accept_macos import Acceptance


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


class EditorAcceptance(Acceptance):
    def exercise(self):
        self.report = self.root / "editor-format-results.json"
        self.results = {"checks": [], "scope": __doc__}
        profile = json.loads((self.fixture / "profile.json").read_text())["profile"]
        assert profile["host"] in ["127.0.0.1", "::1", "localhost"]
        remote = Path(json.loads((self.fixture / "fixture.json").read_text())["root"]).resolve()
        path = remote / "editor-format-owned.txt"
        original = "中😀  \r\n尾\t\r\n" + "  line \t\r\n" * 100 + "\n"
        path.write_bytes(original.encode())
        self.wait(lambda s: len(s["tabs"]) == 1, "Fresh isolated workbench")
        self.action("profile", profile=profile)
        self.action("submit_profile", connect=True)
        self.wait(lambda s: s["modal"] == "trust", "Loopback identity confirmation")
        self.action("trust")
        self.authenticate_fixture()
        self.wait(lambda s: self.pane(s)["state"] == "Connected", "Real SSH connected")
        self.action("open", path=str(path))
        self.wait(lambda s: s.get("editor") and s["editor"]["sha256"] == digest(original), "Real file loaded without automatic formatting")
        self.action("draw")
        self.action("editor_view", start=0, end=0, reversed=False, y=0)
        self.action("editor_insert", text="x")
        before_text = "x" + original
        self.wait(lambda s: s["editor"]["sha256"] == digest(before_text), "Adjacent typing is a real history edit")
        # Preserve a reversed Unicode selection at a genuinely scrolled position.
        self.action("editor_view", start=4, end=18, reversed=True, y=-180)
        self.action("draw")
        before = self.action("snapshot")["editor"]
        assert before["scroll_y"] < 0, before
        self.action("editor_format")
        formatted = "x中😀\n尾\n" + "  line\n" * 100 + "\n"
        state = self.wait(lambda s: not s["editor"]["formatting"] and s["editor"]["sha256"] == digest(formatted), "Formatting applies only the specified cleanup")
        after = state["editor"]
        self.check(after["selection"] == [4, 13] and after["reversed"], "Both Unicode selection endpoints and direction preserved")
        self.check(after["scroll_y"] == before["scroll_y"], "Formatting preserves the actual scroll offset")
        self.check(path.read_bytes() == original.encode() and after["dirty"], "Formatting is dirty and never saves implicitly")
        self.action("keystroke", key="cmd-z")
        undone = self.wait(lambda s: s["editor"]["sha256"] == digest(before_text), "One Undo reverses formatting without undoing preceding typing")["editor"]
        self.check(undone["selection"] == before["selection"] and undone["reversed"] and undone["scroll_y"] == before["scroll_y"], "Undo restores the prior selection and scroll")
        self.action("keystroke", key="cmd-shift-z")
        self.wait(lambda s: s["editor"]["sha256"] == digest(formatted), "Redo reapplies the single formatting transaction")
        self.action("editor_insert", text="new")
        self.action("keystroke", key="cmd-z")
        self.wait(lambda s: s["editor"]["sha256"] == digest(formatted), "Following typing undoes independently of formatting")
        self.action("keystroke", key="escape")
        self.wait(lambda s: s["modal"] is None and s["editor"]["dirty"], "Esc folds the editor while retaining its draft")
        self.action("open", path=str(path))
        self.wait(lambda s: s["modal"] == "editor" and s["editor"]["sha256"] == digest(formatted), "Reopen retains the same unsaved draft")
        self.action("editor_compose", text="组合")
        composing = self.action("snapshot")["editor"]["sha256"]
        self.action("editor_format")
        self.check(self.action("snapshot")["editor"]["sha256"] == composing, "Active IME composition prevents formatting")
        self.action("editor_unmark")
        self.action("close_document")
        self.wait(lambda s: s["modal"] == "close", "Discard remains an explicit confirmation")
        self.action("discard_close")
        self.wait(lambda s: not self.pane(s)["documents"], "Confirmed discard removes the draft")
        self.check(path.read_bytes() == original.encode(), "Discard leaves remote bytes unchanged")
        print(json.dumps({"checks": len(self.results["checks"]), "report": str(self.report)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    args = parser.parse_args()
    EditorAcceptance(args.directory, args.fixture).exercise()


if __name__ == "__main__":
    main()
