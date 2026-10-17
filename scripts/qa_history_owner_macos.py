#!/usr/bin/env python3
"""Verify history submission ownership on isolated real macOS PTYs."""
import argparse
import copy
import json
import os
from pathlib import Path
import shlex
import subprocess
import uuid

from qa_overview_layout_macos import Driver


def pane_at(state, tab, pane):
    return state["tabs"][tab]["panes"][pane]


def ready(state, tab, pane):
    current = pane_at(state, tab, pane)
    return (current["state"] == "Connected" and current["terminal"]
            and current["terminal"]["command_editing"])


class HistoryAcceptance:
    """Use file barriers and terminal output to distinguish rejection from lost input."""
    def __init__(self, driver, directory):
        self.driver = driver
        self.directory = directory
        self.live = []
        self.checks = []

    def focus(self, tab, pane):
        self.driver.action("tab", index=tab)
        self.driver.action("focus_pane", index=pane)

    def barrier(self, tab, pane):
        self.focus(tab, pane)
        marker = "barrier-" + uuid.uuid4().hex
        self.driver.action("type", text=f"\rprintf done > {marker}\r")
        return self.driver.wait(lambda state: (self.directory / marker).is_file()
                                and ready(state, tab, pane), "PTY completion barrier")

    def prepare(self, tab, pane, label):
        self.focus(tab, pane)
        self.driver.wait(lambda state: ready(state, tab, pane), "reliable shell prompt")
        marker = "prepared-" + uuid.uuid4().hex
        self.driver.action("type", text=(f"cd {shlex.quote(str(self.directory))}; "
                                         f"export HIST_OWNER_QA={shlex.quote(label)}; "
                                         f"printf done > {marker}\r"))
        self.driver.wait(lambda state: (self.directory / marker).is_file()
                         and ready(state, tab, pane), "isolated shell directory")
        if (tab, pane) not in self.live:
            self.live.append((tab, pane))

    def open(self, tab=1, pane=0):
        self.focus(tab, pane)
        self.driver.action("history_open")
        state = self.driver.draw_dialog()
        assert state["modal"] == "history"
        assert state["history_view"]["can_insert"], state["notice"]
        return state["history_view"]["target"]

    def invoke(self, target, marker, execute):
        return self.driver.action("history_invoke", target=target, execute=execute,
                                  command=f'printf "%s" "$HIST_OWNER_QA" >> {marker}')

    def assert_no_delivery(self, markers):
        for tab, pane in self.live:
            self.barrier(tab, pane)
        state = self.driver.action("snapshot")
        for marker in markers:
            assert not (self.directory / marker).exists(), marker
            for tab in state["tabs"]:
                for pane in tab["panes"]:
                    terminal = pane["terminal"]
                    text = terminal["text"] if terminal else ""
                    assert marker not in "".join(text.split()), marker

    def reject(self, target, reason, drain=True):
        before = self.driver.action("snapshot")
        markers = []
        for execute in (False, True):
            marker = "rejected-" + uuid.uuid4().hex[:12]
            markers.append(marker)
            state = self.invoke(target, marker, execute)
            assert state["modal"] == before["modal"], reason
            assert state["history_view"] is None or (
                state["history_view"]["target"] == before["history_view"]["target"])
            assert state["notice"], reason
        if drain:
            self.assert_no_delivery(markers)
            self.checks.append(reason + ": insert and execute deliver no command to any tested PTY")
        return markers

    def run(self):
        driver = self.driver
        driver.wait(lambda state: len(state["tabs"]) == 1, "startup")
        driver.action("local", shell="/bin/bash")
        driver.wait(lambda state: len(state["tabs"]) == 2 and ready(state, 1, 0), "Bash PTY")
        self.prepare(1, 0, "original")
        driver.action("split", vertical=False)
        driver.wait(lambda state: len(state["tabs"][1]["panes"]) == 2
                    and ready(state, 1, 1), "second real pane")
        self.prepare(1, 1, "other-pane")

        target = self.open()
        marker = "positive-insert"
        before_count = driver.action("snapshot")["history_count"]
        state = self.invoke(target, marker, False)
        assert state["modal"] is None
        driver.wait(lambda state: marker in "".join(
            pane_at(state, 1, 0)["terminal"]["text"].split()), "insert echoed without Enter")
        assert not (self.directory / marker).exists()
        assert driver.action("snapshot")["history_count"] == before_count
        self.checks.append("valid insert reaches the captured PTY without executing or recording history")
        driver.action("type", text="\r")
        driver.wait(lambda state: (self.directory / marker).is_file() and ready(state, 1, 0),
                    "explicit Enter executes inserted command")
        assert (self.directory / marker).read_text() == "original"
        self.checks.append("only a subsequent explicit Enter executes the inserted command")

        target = self.open()
        marker = "positive-execute"
        self.invoke(target, marker, True)
        driver.wait(lambda state: (self.directory / marker).is_file() and ready(state, 1, 0),
                    "history execution completes")
        assert (self.directory / marker).read_text() == "original"
        assert marker not in "".join(pane_at(driver.action("snapshot"), 1, 1)["terminal"]["text"].split())
        self.checks.append("valid execute submits once to the original PTY and leaves the other pane untouched")

        target = self.open()
        self.focus(1, 1)
        state = driver.draw_dialog()
        assert state["history_view"]["target"] == target
        assert not state["history_view"]["can_insert"]
        self.reject(target, "switching panes preserves the old target and rejects submission")
        driver.action("dismiss")

        driver.action("local", shell="/bin/bash")
        driver.wait(lambda state: len(state["tabs"]) == 3 and ready(state, 2, 0), "second Bash tab")
        self.prepare(2, 0, "other-tab")
        target = self.open()
        self.focus(2, 0)
        state = driver.draw_dialog()
        assert state["history_view"]["target"] == target
        self.reject(target, "switching tabs cannot retarget a rendered history action")
        driver.action("dismiss")

        old = self.open()
        driver.action("dismiss")
        target = self.open()
        assert old["instance"] != target["instance"] and old["owner"] == target["owner"]
        self.reject(old, "reopening history invalidates callbacks from the old modal instance")
        driver.action("dismiss")
        self.focus(1, 0)
        self.reject(target, "a closed history modal cannot submit a delayed callback")

        target = self.open()
        driver.action("settings")
        driver.draw_dialog()
        self.reject(target, "a hidden history parent cannot submit behind Settings")
        driver.action("cancel_modal")
        state = driver.draw_dialog()
        assert state["history_view"]["target"] == target
        assert not state["history_view"]["can_insert"]  # barriers left the other tab active
        self.focus(1, 0)
        assert driver.action("snapshot")["history_view"]["can_insert"]
        self.checks.append("restoring a parent preserves its instance and requires its original active owner")
        driver.action("dismiss")

        target = self.open()
        driver.action("history_query", query="positive-execute")
        driver.wait(lambda state: len(state["history_view"]["rows"]) == 1, "persisted history row")
        driver.action("history_select", index=0)
        driver.action("history_review_delete")
        driver.wait(lambda state: state["modal"] == "delete_history", "delete confirmation")
        self.focus(1, 1)
        driver.action("cancel_modal")
        state = driver.draw_dialog()
        assert state["history_view"]["target"] == target
        assert len(state["history_view"]["selected"]) == 1
        assert not state["history_view"]["can_insert"]
        self.checks.append("cancelling deletion restores the original history instance, query and selection")
        self.reject(target, "restoring deletion after a pane switch does not change the execution target")
        driver.action("history_query", query="")
        driver.action("dismiss")

        target = self.open()
        wrong = copy.deepcopy(target)
        wrong["owner"]["attempt"] = str(uuid.uuid4())
        self.reject(wrong, "an obsolete connection attempt cannot submit")
        driver.action("dismiss")

        target = self.open()
        driver.action("reconnect")
        driver.wait(lambda state: ready(state, 1, 0)
                    and pane_at(state, 1, 0)["attempt"] != target["owner"]["attempt"],
                    "new real connection attempt")
        self.prepare(1, 0, "reconnected")
        state = driver.action("snapshot")
        assert state["history_view"]["target"] == target
        assert not state["history_view"]["can_insert"]
        self.reject(target, "a real reconnect invalidates the old modal owner")
        driver.action("dismiss")

        self.focus(1, 0)
        driver.action("type", text="sleep 30\r")
        driver.wait(lambda state: not pane_at(state, 1, 0)["terminal"]["command_editing"],
                    "foreground program leaves the reliable prompt")
        driver.action("history_open")
        state = driver.draw_dialog()
        assert not state["history_view"]["can_insert"]
        markers = self.reject(state["history_view"]["target"], "foreground program", drain=False)
        driver.action("type", text="\x03")
        driver.wait(lambda state: ready(state, 1, 0), "foreground program interrupted")
        self.assert_no_delivery(markers)
        self.checks.append("foreground programs receive neither inserted text nor an execution request")
        driver.action("dismiss")

        target = self.open()
        driver.action("type", text="exit\r")
        driver.wait(lambda state: pane_at(state, 1, 0)["state"] != "Connected", "PTY exit")
        self.live.remove((1, 0))
        self.reject(target, "a disconnected terminal cannot accept history input")
        driver.action("dismiss")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    env = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
           "MANTASH_QA_CONTROL": str(directory / "command.json"),
           "MANTASH_QA_BACKGROUND": "1", "MANTASH_QA_HIDDEN": "1"}
    with (directory / "stdout.log").open("w") as stdout, (directory / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=directory, env=env,
                                   stdout=stdout, stderr=stderr)
        (directory / "process.json").write_text(json.dumps({"pid": process.pid}) + "\n")
        driver = Driver(directory, process)
        acceptance = HistoryAcceptance(driver, directory)
        try:
            acceptance.run()
        finally:
            if process.poll() is None:
                driver.send("quit")
                try:
                    process.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=10)
    assert process.returncode == 0, process.returncode
    report = {"checks": acceptance.checks, "binary": str(args.binary.resolve()),
              "scope": "isolated real PTYs and production history handlers; no physical input or SSH transport"}
    result = directory / "history-owner-results.json"
    result.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"checks": len(acceptance.checks), "report": str(result)}))


if __name__ == "__main__":
    main()
