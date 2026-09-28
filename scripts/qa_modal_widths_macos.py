#!/usr/bin/env python3
"""Check actual native dialog widths using one isolated debug process."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from qa_history_connection_modals_macos import history_entries
from qa_overview_layout_macos import Driver


def measured_dialog(driver, key, expected, checks):
    """Require fresh native width and centered x/y coordinates in the window."""
    driver.action("draw")
    driver.action("draw")
    state = driver.wait(
        lambda state: state["modal"] == key
        and state["dialog_frame_bounds"]
        and abs(state["dialog_frame_bounds"]["width"] + 2 - expected) < 1,
        f"{key} frame width {expected}",
    )
    frame = state["dialog_frame_bounds"]
    assert frame["x"] >= 0 and frame["x"] + frame["width"] <= state["width"]
    assert abs(frame["x"] + frame["width"] / 2 - state["width"] / 2) <= 1, (key, frame, state["width"])
    assert abs(frame["y"] + frame["height"] / 2 - state["height"] / 2) <= 1, (key, frame, state["height"])
    checks.append(f"{key}: {expected}px and centered")
    return state


def main():
    """Exercise ordinary and confirmation dialogs without remote IO."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
                   "MANTASH_QA_CONTROL": str(directory / "command.json"),
                   "MANTASH_QA_BACKGROUND": "1"}
    process = subprocess.Popen(
        [str(args.binary.resolve())], cwd=directory, env=environment,
        stdout=(directory / "stdout.log").open("w"),
        stderr=(directory / "stderr.log").open("w"),
    )
    driver = Driver(directory, process)
    checks = []
    try:
        driver.wait(lambda state: len(state["tabs"]) == 1, "startup")
        driver.action("history_fixture", entries=history_entries()[:4])
        measured_dialog(driver, "history", 640, checks)
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "history closed")


        driver.action("transfer_fixture", confirm_upload=True)
        measured_dialog(driver, "transfer", 480, checks)
        driver.action("transfer_fixture", confirm_upload=True, download=True)
        measured_dialog(driver, "transfer", 480, checks)
        driver.action("resize", width=960, height=640)
        driver.wait(lambda state: state["width"] == 960 and state["height"] == 640, "minimum window size")
        driver.action("draw")
        driver.action("draw")
        resized = driver.wait(lambda state: state["modal"] == "transfer" and state["dialog_frame_bounds"]
                              and abs(state["dialog_frame_bounds"]["x"] + state["dialog_frame_bounds"]["width"] / 2 - 480) <= 1
                              and abs(state["dialog_frame_bounds"]["y"] + state["dialog_frame_bounds"]["height"] / 2 - 320) <= 1,
                              "transfer recentered after window resize")
        checks.append("transfer recentered at 960x640 without reopening")
        driver.action("transfer_fixture", confirm_upload=True, failed=True)
        measured_dialog(driver, "transfer", 480, checks)
        driver.action("resize", width=1280, height=800)
        driver.wait(lambda state: state["width"] == 1280 and state["height"] == 800, "restored window size")
        driver.action("transfer_fixture", confirm_upload=False)
        measured_dialog(driver, "transfers", 640, checks)
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "transfers closed")

        driver.action("open_system_tools", page="processes")
        measured_dialog(driver, "system_tools", 640, checks)
        driver.action("open_process_details")
        measured_dialog(driver, "process_details", 640, checks)
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] == "system_tools", "system tools restored")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "system tools closed")

        driver.action("sidebar")
        library = measured_dialog(driver, "connections", 640, checks)
        profile_id = library["connection_library"]["visible"][0]
        driver.action("clone_profile", id=profile_id)
        measured_dialog(driver, "profile", 560, checks)
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] == "connections", "connections restored")
        driver.action("import", text="name,host,port,username,password\nQA fixture,127.0.0.1,22,qa,\n")
        measured_dialog(driver, "import", 480, checks)
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] == "connections", "import cancelled")
        driver.action("keystroke", key="escape")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "connections closed")

        driver.action("credential_fixture")
        measured_dialog(driver, "credentials", 360, checks)
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "credentials closed")
    finally:
        if process.poll() is None:
            driver.send("quit")
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
    assert process.returncode == 0, process.returncode
    report = {"checks": checks, "binary": str(args.binary.resolve())}
    (directory / "dialog-widths.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"checks": checks, "report": str(directory / "dialog-widths.json")}))


if __name__ == "__main__":
    main()
