#!/usr/bin/env python3
"""Exercise an already-running opt-in native QA instance and its loopback SSH fixture.

This script changes only the isolated QA workspace. It does not synthesize OS input,
and therefore does not replace mouse, IME, clipboard or platform acceptance.
"""
import argparse
import json
import os
from pathlib import Path
import time


def main():
    """Keep async assertions separate from command acknowledgement and persist concrete evidence."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    args = parser.parse_args()
    root = args.directory
    fixture = json.loads((args.fixture / "fixture.json").read_text())
    results = []

    def read():
        """Read the driver's atomic snapshot, containing no credential values."""
        return json.loads((root / "state.json").read_text())

    def wait(predicate, description):
        """Allow real network and PTY events to settle, never treating a timeout as success."""
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            state = read()
            if predicate(state):
                results.append(description)
                return state
            time.sleep(0.1)
        raise AssertionError(description)

    def action(name, **values):
        """Write a fresh request and wait for the corresponding native acknowledgement."""
        sequence = time.time_ns()
        tmp = root / "command.tmp"
        tmp.write_text(json.dumps({"sequence": sequence, "action": name, **values}))
        os.replace(tmp, root / "command.json")
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            state = read()
            if state["sequence"] >= sequence:
                return state
            time.sleep(0.05)
        raise AssertionError(f"No acknowledgement: {name}")

    def pane(state):
        """Select only within the active QA tab."""
        tab = state["tabs"][state["active_tab"]]
        return tab["panes"][tab["active_pane"]]

    action("tab", index=0)
    state = wait(lambda s: len(s["tabs"][0]["panes"]) == 5 and all(p["state"] == "Connected" for p in s["tabs"][0]["panes"]), "restored five local shells")
    assert state["tool_width"] is None
    action("split", vertical=True)
    assert len(read()["tabs"][0]["panes"]) == 5
    survivors = {p["owner"] for p in read()["tabs"][0]["panes"][:4]}
    action("close_pane", index=4)
    assert {p["owner"] for p in read()["tabs"][0]["panes"]} == survivors
    action("split", vertical=True)
    assert len(read()["tabs"][0]["panes"]) == 5
    results.append("split cap and close preserve surviving owners")
    action("focus_pane", index=0)
    action("type", text="printf 'MantaSH 中文验收\\n'\r")
    wait(lambda s: "MantaSH 中文验收" in pane(s)["terminal"]["text"], "real local Unicode output")
    action("search", query="MantaSH 中文验收")
    wait(lambda s: pane(s)["terminal"]["matches"] > 0, "Unicode search retains spaces")
    action("focus_pane", index=1)
    action("search", query="no-such-mantash-output")
    action("focus_pane", index=0)
    assert pane(read())["terminal"]["matches"] > 0
    results.append("per-pane search survives focus changes")
    action("tab", index=1)
    if pane(read())["state"] != "Connected":
        action("reconnect")
    state = wait(lambda s: pane(s)["state"] == "Connected", "real loopback SSH reconnect")
    owner, attempt = pane(state)["owner"], pane(state)["attempt"]
    title = state["tabs"][1]["title"]
    action("split", vertical=False)
    assert len(read()["tabs"][1]["panes"]) == 1
    results.append("SSH refuses split requests")
    action("resize", width=1440, height=900)
    action("panel_width", width=680)
    wait(lambda s: s["tool_width"] == 680, "preferred tool width is applied")
    action("resize", width=960, height=640)
    wait(lambda s: s["tool_width"] == 480 and s["preferences"]["tool_preferred_width"] == 680, "temporary 50 percent cap preserves preference")
    action("resize", width=1440, height=900)
    wait(lambda s: s["tool_width"] == 680, "window growth restores tool preference")
    action("hide_tool")
    assert read()["tool_width"] is None
    action("tool", tool="files")
    assert read()["tool_width"] == 680
    action("navigate", path=fixture["root"])
    wait(lambda s: "README.md" in pane(s)["files"], "real SFTP directory listing")
    action("open", path=fixture["root"] + "/README.md")
    wait(lambda s: len(pane(s)["documents"]) == 1, "remote document opens in right editor")
    action("edit", text="# MantaSH v2\n\n真实 SSH / SFTP 文本保存。\n")
    wait(lambda s: pane(s)["documents"][0]["dirty"], "editor tracks draft changes")
    action("tool", tool="files")
    action("tab", index=0)
    assert read()["tool_width"] is None
    action("tab", index=1)
    action("tool", tool="editor")
    assert pane(read())["documents"][0]["dirty"]
    results.append("draft survives tools and tabs; local tool remains hidden")
    action("hide_tool")
    assert read()["tool_width"] is None
    action("reopen_tool")
    assert pane(read())["tool"] == "editor" and pane(read())["documents"][0]["dirty"]
    results.append("reopening restores the editor and its unsaved draft")
    action("save")
    wait(lambda s: not pane(s)["documents"][0]["dirty"] and not pane(s)["documents"][0]["saving"], "successful SFTP save clears dirty marker")
    assert "真实 SSH" in Path(fixture["root"], "README.md").read_text()
    action("font_sizes", ui=20, terminal=17)
    action("language", english=True)
    action("resize", width=960, height=640)
    state = wait(lambda s: s["width"] == 960 and s["preferences"]["terminal_size"] == 17, "large odd font sizes applied at small window")
    assert pane(state)["owner"] == owner and pane(state)["attempt"] == attempt
    assert state["tabs"][1]["title"] == title
    results.append("font and panel changes preserve connection identity and SSH title")
    action("system_page", page="overview")
    action("refresh_monitor")
    wait(lambda s: pane(s)["monitor"] and "system" in pane(s)["monitor"]["errors"], "macOS SSH host explicitly rejects Linux monitoring")
    action("font_sizes", ui=14, terminal=12)
    action("language", english=False)
    action("resize", width=1280, height=800)
    action("panel_width", width=None)
    action("tool", tool="editor")
    evidence={"date":"2026-09-09","source":"native opt-in QA driver","checks":results,"count":len(results),"pid":read()["pid"],"limits":["No synthetic OS input or IME testing","Loopback server runs macOS; real Linux and Windows acceptance remain separate"]}
    (root/"smoke-results.json").write_text(json.dumps(evidence,ensure_ascii=False,indent=2))
    print(json.dumps(evidence,ensure_ascii=False,indent=2))


if __name__ == "__main__":
    main()
