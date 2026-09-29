#!/usr/bin/env python3
"""Check the native process-list dialog at sparse and dense sample sizes."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from qa_overview_layout_macos import Driver


def draw(driver):
    """Wait for fresh GPUI layout without synthesizing keyboard input."""
    driver.action("draw")
    driver.action("draw")
    return driver.action("snapshot")


def assert_frame(state):
    """The table must stay centered and within the existing 640px modal width."""
    frame = state["dialog_frame_bounds"]
    assert abs(frame["width"] + 2 - 640) <= 1, frame
    assert abs(frame["x"] + frame["width"] / 2 - state["width"] / 2) <= 1, frame
    assert abs(frame["y"] + frame["height"] / 2 - state["height"] / 2) <= 1, frame


def assert_columns(view):
    """Compare painted column bounds, not source-level width declarations."""
    cells = view["geometry"]
    for key in ("pid", "name", "user", "cpu", "memory"):
        header = cells[f"header_{key}"]
        row = cells[f"row_{key}"]
        assert abs(header["x"] - row["x"]) <= 1, (key, header, row)
        assert abs(header["width"] - row["width"]) <= 1, (key, header, row)
    assert cells["toolbar"]["y"] < cells["columns"]["y"] < cells["viewport"]["y"]
    assert 20 <= cells["first_row"]["height"] <= 48
    assert view["max_x"] <= 1, view["max_x"]


def main():
    """Use isolated data and fixture samples; no real SSH or process signals."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
                   "MANTASH_QA_CONTROL": str(directory / "command.json"), "MANTASH_QA_BACKGROUND": "1"}
    with (directory / "stdout.log").open("w") as stdout, (directory / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=directory, env=environment,
                                   stdout=stdout, stderr=stderr)
        driver = Driver(directory, process)
        checks = []
        try:
            driver.wait(lambda state: bool(state["tabs"]), "startup")
            driver.action("resize", width=1280, height=800)
            for ui, night in ((14, False), (18, True)):
                driver.action("font_sizes", ui=ui, terminal=12)
                driver.action("theme", night=night)
                driver.action("process_fixture", mode="ready")
                driver.action("cancel_modal")
                driver.wait(lambda s: s["modal"] == "system_tools", "process list")
                driver.action("process_list_state", query="", y=0)
                driver.action("process_list_fixture", count=3)
                few = draw(driver)
                assert_frame(few)
                assert few["process_list"]["total"] == 3
                commands = [f"/usr/bin/node qa-helper-{i} --mode=worker-{i}" for i in range(3)]
                assert few["process_list"]["visible_commands"] == commands
                for english, caption in ((False, "命令"), (True, "Command")):
                    driver.action("language", english=english)
                    localized = draw(driver)
                    assert localized["process_list"]["command_header"] == caption
                for query, expected in (("NODE", commands), ("--mode=worker-1", [commands[1]]),
                                        ("qa-owner", []), ("50000", []), ("not-present", []), ("", commands)):
                    driver.action("process_list_state", query=query, y=0)
                    result = draw(driver)
                    assert result["process_list"]["visible_commands"] == expected, (query, result["process_list"])
                checks.append("Command header localized; full command arguments shown and matched; user/PID-only and absent keywords return no rows")
                assert few["dialog_frame_bounds"]["height"] < 300, few["dialog_frame_bounds"]
                assert_columns(few["process_list"])
                viewport = few["process_list"]["bounds"]
                first = few["process_list"]["geometry"]["first_row"]
                assert first["y"] >= viewport["y"] - 1
                assert first["y"] + 3 * first["height"] <= viewport["y"] + viewport["height"] + 1, (first, viewport)
                assert few["process_list"]["max_y"] <= 1
                checks.append(f"{ui}px {'dark' if night else 'light'}: three aligned rows, compact centered frame")
                driver.action("process_list_state", query="", y=0)
                driver.action("process_list_fixture", count=90)
                many = draw(driver)
                assert_frame(many)
                assert many["process_list"]["total"] == 90
                assert many["process_list"]["max_y"] > 100, many["process_list"]
                assert many["dialog_frame_bounds"]["height"] > few["dialog_frame_bounds"]["height"] + 150
                assert_columns(many["process_list"])
                scrollbar = many["process_list"]["geometry"]["scrollbar"]
                driver.action("process_list_state", query="", y=-120)
                scrolled = draw(driver)
                scrolled_bar = scrolled["process_list"]["geometry"]["scrollbar"]
                assert scrolled_bar == scrollbar, (scrollbar, scrolled_bar)
                checks.append("ninety rows scroll inside the list with fixed toolbar and aligned headers")
                driver.action("process_list_state", query="qa-helper-1", y=-120)
                filtered = draw(driver)
                assert filtered["process_list"]["query"] == "qa-helper-1"
                assert filtered["process_list"]["sort"] == "cpu" and filtered["process_list"]["descending"]
                assert filtered["dialog_frame_bounds"]["height"] == many["dialog_frame_bounds"]["height"]
                assert_columns(filtered["process_list"])
                checks.append("filter and sorting preserve list frame and column alignment")
                driver.action("process_list_fixture", count=0)
                empty = draw(driver)
                assert_frame(empty)
                assert empty["process_list"]["total"] == 0
                assert empty["dialog_frame_bounds"]["height"] < 260
                checks.append("empty sample remains distinct from a populated list")
        finally:
            if process.poll() is None:
                driver.send("quit")
                process.wait(timeout=20)
    assert process.returncode == 0, process.returncode
    report = {"checks": checks, "binary": str(args.binary.resolve()), "preview_signal_blocked": True}
    (directory / "process-list-results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"checks": len(checks), "report": str(directory / "process-list-results.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
