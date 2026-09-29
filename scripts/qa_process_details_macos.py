#!/usr/bin/env python3
"""Inspect process details in an isolated, signal-free native macOS window."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from qa_overview_layout_macos import Driver


def draw(driver):
    """Force fresh layout frames without sending input to the terminal."""
    driver.action("draw")
    driver.action("draw")
    return driver.action("snapshot")


def centered(state, width=640):
    """Assert measured GPUI frame position and size against the actual viewport."""
    box = state["dialog_frame_bounds"]
    assert abs(box["width"] + 2 - width) <= 1, box
    assert abs(box["x"] + box["width"] / 2 - state["width"] / 2) <= 1, box
    assert abs(box["y"] + box["height"] / 2 - state["height"] / 2) <= 1, box


def main():
    """Run fixture-only actions in one fresh debug process and preserve results in scratch."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
                   "MANTASH_QA_CONTROL": str(directory / "command.json"), "MANTASH_QA_BACKGROUND": "1"}
    process = subprocess.Popen([str(args.binary.resolve())], cwd=directory, env=environment,
                               stdout=(directory / "stdout.log").open("w"),
                               stderr=(directory / "stderr.log").open("w"))
    driver = Driver(directory, process)
    checks = []
    try:
        driver.wait(lambda s: len(s["tabs"]) == 1, "startup")
        for ui, night in ((12, False), (14, False), (18, True)):
            driver.action("font_sizes", ui=ui, terminal=12)
            driver.action("theme", night=night)
            driver.action("process_fixture", mode="ready")
            state = draw(driver)
            view = state["process_view"]
            assert state["modal"] == "process_details" and view["preview"]
            assert view["metrics"]["user"] == "qa-user" and view["metrics"]["parent"] == 99
            assert view["metrics"]["cpu"] == 12.5 and view["metrics"]["rss"] == 2_097_152
            assert view["command_len"] > 0 and view["raw_len"] > 1000
            assert not view["signal_enabled"] and view["action_reason"] == "process_preview_only"
            assert view["footer_bounds"] and state["modal_scroll"]["bounds"]["height"] > 100
            centered(state)
            geometry = view["geometry"]
            identity, metrics, command = (geometry[f"detail_{name}"] for name in ("identity", "metrics", "command"))
            assert state["modal_scroll"]["max_x"] <= 1, "detail must not scroll horizontally"
            assert identity["height"] <= 32, identity
            assert metrics["height"] <= 3 * ui * 1.75 + 16, metrics
            assert abs(metrics["y"] - identity["y"] - identity["height"] - 12) <= 1
            assert abs(command["y"] - metrics["y"] - metrics["height"] - 12) <= 1
            assert command["y"] - identity["y"] < 160, geometry
            checks.append(f"{ui}px {'dark' if night else 'light'} detail metrics, safe controls and centered frame")

        driver.action("resize", width=960, height=640)
        driver.wait(lambda s: s["width"] == 960 and s["height"] == 640, "minimum viewport")
        before_refresh = draw(driver)
        driver.action("process_refresh")
        refreshing = driver.wait(lambda s: s["process_view"] and s["process_view"]["refreshing"]
                                 and s["process_view"]["command_len"] > 0, "old command retained during refresh")
        refreshing = draw(driver)
        assert refreshing["modal_scroll"] == before_refresh["modal_scroll"], "refresh must not shift the content"
        request = refreshing["process_view"]["request"]
        driver.action("process_reply", stale=True, error=True)
        old = driver.action("snapshot")["process_view"]
        assert old["refreshing"] and old["request"] == request and old["refresh_error"] is None
        driver.action("process_reply", error=True)
        failed = driver.wait(lambda s: s["process_view"] and not s["process_view"]["refreshing"]
                             and s["process_view"]["refresh_error"] == "QA refresh failed", "refresh error retains old content")
        assert failed["process_view"]["command_len"] == old["command_len"]
        driver.action("process_refresh")
        driver.action("process_reply", long=True)
        long_view = draw(driver)
        assert long_view["process_view"]["command_len"] > 1000
        assert long_view["process_view"]["long_command"]
        assert long_view["modal_scroll"]["max_x"] <= 1
        assert long_view["modal_scroll"]["max_y"] > failed["modal_scroll"]["max_y"] + 100
        centered(long_view)
        checks.append("latest request wins; full long command is visible without expanding or horizontal overflow")

        footer = long_view["process_view"]["footer_bounds"]
        driver.action("scroll_modal", y=-100000)
        scrolled = draw(driver)
        assert scrolled["modal_scroll_y"] < -100
        assert abs(scrolled["process_view"]["footer_bounds"]["y"] - footer["y"]) <= 1
        centered(scrolled)
        checks.append("full command and supplementary status scroll together within a fixed header and footer")

        for mode, issue in (("reused", "process_changed"), ("reboot", "process_changed"),
                            ("gone", "process_gone"), ("sample_error", "process_sample_unavailable"),
                            ("stale", "process_sample_unavailable"), ("offline", "process_offline")):
            driver.action("process_sample_fixture", mode=mode)
            state = draw(driver)
            assert state["process_view"]["target_reason"] == issue, (mode, state["process_view"])
            assert state["process_view"]["metrics"] is None
            assert not state["process_view"]["signal_enabled"]
            checks.append(f"{mode} never renders another process as the original")
        driver.action("process_fixture", mode="missing")
        missing = draw(driver)["process_view"]
        assert missing["target_reason"] == "process_identity_missing" and missing["read_error"]
        driver.action("process_fixture", mode="error")
        read_error = draw(driver)["process_view"]
        assert read_error["read_error"] == "QA detail read failed"
        driver.action("process_fixture", mode="loading")
        loading = draw(driver)["process_view"]
        assert loading["refreshing"] and loading["command_len"] is None
        driver.action("process_reply")
        driver.wait(lambda s: s["process_view"] and s["process_view"]["command_len"] > 0,
                    "loading completes from latest reply")
        checks.append("missing identity, read error and loading states remain explicit")

        driver.action("process_fixture", mode="ready", long=True)
        draw(driver)
        driver.action("scroll_modal", y=-140)
        before = draw(driver)
        old_offset = before["modal_scroll_y"]
        assert old_offset < -10
        driver.action("process_confirm_fixture", force=False)
        term = driver.wait(lambda s: s["modal"] == "process_confirm" and s["process_view"]
                           and s["modal_navigation"]["confirmation_parent"] == "process_details",
                           "TERM confirmation retains detail parent")
        assert term["process_view"]["confirm"] == {"host": "127.0.0.1:1", "action": "terminate"}
        assert not term["process_view"]["signal_enabled"] and term["process_view"]["attempt"] is None
        driver.action("process_confirm_submit_fixture")
        after_preview_submit = driver.action("snapshot")
        assert after_preview_submit["modal"] == "process_confirm" and after_preview_submit["process_view"]["attempt"] is None
        driver.action("cancel_modal")
        restored = draw(driver)
        assert restored["modal"] == "process_details"
        assert restored["process_view"]["command_len"] == before["process_view"]["command_len"]
        assert abs(restored["modal_scroll_y"] - old_offset) <= 2
        checks.append("TERM confirmation is signal-free; cancel restores detail text and scroll")
        driver.action("process_confirm_fixture", force=True)
        kill = driver.wait(lambda s: s["modal"] == "process_confirm"
                           and s["process_view"] and s["process_view"]["confirm"], "KILL confirmation")
        assert kill["process_view"]["confirm"]["action"] == "force"
        driver.action("cancel_modal")
        driver.wait(lambda s: s["modal"] == "process_details", "KILL cancellation returns to detail")
        checks.append("SIGKILL confirmation remains separate and cancelable")

        for phase in ("sending", "sent", "gone", "still_running", "denied", "changed", "unknown"):
            driver.action("process_attempt_fixture", phase=phase)
            observed = draw(driver)["process_view"]
            assert observed["attempt"] == ("process_running" if phase == "still_running" else f"process_{phase}")
            assert not observed["signal_enabled"]
        driver.action("cancel_modal")
        back = driver.wait(lambda s: s["modal"] == "system_tools" and s["modal_stack"] == [],
                           "detail closes back to processes")
        checks.append("send/sent/gone/timeout/denied/changed/unknown stay distinct; detail returns to list")
        driver.action("process_list_state", query="qa", y=-120)
        listed = draw(driver)
        list_state = listed["process_list"]
        assert list_state["query"] == "qa" and list_state["sort"] == "cpu" and list_state["descending"]
        assert list_state["max_y"] > 120 and list_state["scroll_y"] < -50, list_state
        driver.action("reopen_process_preview")
        driver.wait(lambda s: s["modal"] == "process_details"
                    and s["modal_navigation"]["parents"] == ["system_tools"], "reopen process from list")
        driver.action("cancel_modal")
        returned = draw(driver)
        assert returned["modal"] == "system_tools"
        assert returned["process_list"]["query"] == list_state["query"]
        assert returned["process_list"]["sort"] == list_state["sort"]
        assert abs(returned["process_list"]["scroll_y"] - list_state["scroll_y"]) <= 2
        checks.append("list search, CPU sort and actual scroll survive a detail round trip")
    finally:
        if process.poll() is None:
            driver.send("quit")
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
    assert process.returncode == 0, process.returncode
    report = {"checks": checks, "binary": str(args.binary.resolve()), "preview_signal_blocked": True}
    (directory / "process-results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"checks": len(checks), "report": str(directory / "process-results.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
