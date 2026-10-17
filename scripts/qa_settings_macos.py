#!/usr/bin/env python3
"""Verify production preference actions, persistence and native settings layout in isolation."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sqlite3

from qa_overview_layout_macos import Driver


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {**os.environ, "MANTASH_DATA_DIR": str(root / "data"),
           "MANTASH_QA_CONTROL": str(root / "command.json"),
           "MANTASH_QA_BACKGROUND": "1"}
    env.pop("MANTASH_QA_HIDDEN", None)
    checks = []
    defaults = dict(follow_system_theme=False, mouse_reporting=True,
                    open_files_on_connect=True, remember_credentials=True)
    expected = {key: not value for key, value in defaults.items()}
    for phase in ["exercise", "restore"]:
        with (root / (phase + ".log")).open("w") as log:
            app = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=subprocess.STDOUT)
            driver = Driver(root, app)
            try:
                state = driver.wait(lambda s: s["pid"] == app.pid and bool(s["tabs"]), "New isolated process")
                assert "default_ssh_port" not in state["preferences"]
                driver.action("ssh_form")
                form_state = driver.wait(lambda s: s["modal"] == "profile", "New SSH form")
                assert form_state["connection_library"]["form"]["port"] == "22"
                assert form_state["connection_library"]["form"]["username"] == "root"
                checks.append(f"New connection form uses root and port 22 at {phase} startup")
                driver.action("dismiss")
                if phase == "exercise":
                    assert all(state["preferences"][key] == value for key, value in defaults.items())
                    checks.append("Four switch preferences have their specified defaults")
                    driver.action("import", text="name,host,port,username,password\nSettings fixture,127.0.0.1,2222,qa,\n")
                    driver.action("confirm_import")
                    driver.wait(lambda s: s["profiles"] == 1 and not s["commit"]["busy"], "Real profile transaction")
                    driver.action("settings")
                    for ui in [12, 14, 18]:
                        for night in [False, True]:
                            driver.action("resize", width=960, height=640)
                            driver.action("font_sizes", ui=ui, terminal=ui)
                            driver.action("theme", night=night)
                            state = driver.draw_dialog()
                            bounds = state["dialog_frame_bounds"]
                            expected_width = 520 + max(ui - 14, 0) * 16
                            assert abs(bounds["width"] + 2 - expected_width) <= 1
                            assert bounds and bounds["x"] >= 15 and bounds["y"] >= 15
                            assert bounds["x"] + bounds["width"] <= state["width"] - 15
                            assert bounds["y"] + bounds["height"] <= state["height"] - 15
                            assert state["modal_scroll"]["max_x"] <= 1
                            checks.append(f"Settings fit 960x640 at {ui}px in {'night' if night else 'day'} theme")
                    # Use measured hit areas, then the production AppKit pointer path.
                    driver.action("resize", width=1280, height=900)
                    driver.action("font_sizes", ui=14, terminal=12)
                    driver.draw_dialog()
                    for key in expected:
                        for value in [expected[key], defaults[key]]:
                            state = driver.draw_dialog()
                            bounds = state["settings_switch_bounds"][key]
                            assert abs(bounds["width"] - 44) <= 1
                            assert 24 <= bounds["height"] <= 32
                            viewport = state["modal_scroll"]["bounds"]
                            assert bounds["y"] >= viewport["y"]
                            assert bounds["y"] + bounds["height"] <= viewport["y"] + viewport["height"]
                            center = [bounds["x"] + bounds["width"] / 2,
                                      bounds["y"] + bounds["height"] / 2]
                            driver.action("pointer_gesture", points=[center, center])
                            state = driver.wait(lambda s: s["preferences"][key] == value
                                                and s["settings_switch_focus"] == key,
                                                "Click toggles and focuses " + key)
                            assert state["modal"] == "settings"
                        driver.action("focus_setting", key=key)
                        driver.wait(lambda s: s["settings_switch_focus"] == key, "Switch focused: " + key)
                        driver.action("keystroke", key="space", release=True)
                        driver.wait(lambda s: s["preferences"][key] == expected[key], "Space toggles " + key)
                        driver.action("keystroke", key="enter", release=True)
                        driver.wait(lambda s: s["preferences"][key] == defaults[key], "Enter toggles " + key)
                        driver.action("keystroke", key="space", release=True)
                        driver.wait(lambda s: s["preferences"][key] == expected[key], "Switch reapplied: " + key)
                    checks.append("All four switches toggle and focus through native pointer clicks")
                    checks.append("All four switches toggle once per complete Space/Enter key cycle")
                    driver.action("focus_setting", key="open_files_on_connect")
                    driver.action("keystroke", key="tab")
                    driver.wait(lambda s: s["settings_switch_focus"] == "remember_credentials", "Tab reaches next switch")
                    driver.action("keystroke", key="shift-tab")
                    driver.wait(lambda s: s["settings_switch_focus"] == "open_files_on_connect", "Shift+Tab returns to switch")
                    checks.append("Switches participate in forward and backward modal focus traversal")
                    driver.action("theme", night=False)
                    assert not driver.action("snapshot")["preferences"]["follow_system_theme"]
                    driver.action("toggle_setting", key="follow_system_theme")
                    checks.append("Manual theme choice turns off system-theme following")
                    driver.action("dismiss")
                else:
                    assert all(state["preferences"][key] == value for key, value in expected.items())
                    checks.append("All four switch preferences survive a clean process restart")
                    before = (state["profiles"], len(state["tabs"]), state["width"], state["height"])
                    driver.action("settings")
                    driver.action("reset_settings")
                    state = driver.action("snapshot")
                    assert all(state["preferences"][key] == value for key, value in defaults.items())
                    assert before == (state["profiles"], len(state["tabs"]), state["width"], state["height"])
                    checks.append("Restore defaults preserves profiles, workspace and window dimensions")
                    driver.action("dismiss")
            finally:
                if app.poll() is None:
                    driver.send("quit")
                assert app.wait(timeout=20) == 0
        # The app has exited: inspect only this run's isolated database and seed a legacy preference.
        with sqlite3.connect(root / "data" / "mantash.sqlite3") as database:
            prefs = json.loads(database.execute(
                "SELECT data FROM records WHERE kind='preferences' AND id='current'").fetchone()[0])
            assert "default_ssh_port" not in prefs
            profiles = [json.loads(row[0]) for row in database.execute(
                "SELECT data FROM records WHERE kind='profile'")]
            assert len(profiles) == 1 and profiles[0]["port"] == 2222
            if phase == "exercise":
                prefs["default_ssh_port"] = 2222
                database.execute("UPDATE records SET data=? WHERE kind='preferences' AND id='current'",
                                 (json.dumps(prefs),))
            else:
                checks.append("Legacy default port is ignored on restart and omitted when preferences are saved")
                checks.append("Existing profile keeps custom port 2222 through restart and settings reset")
    report = root / "settings-results.json"
    report.write_text(json.dumps({"checks": checks, "scope": __doc__}, indent=2))
    print(json.dumps({"checks": len(checks), "report": str(report)}))


if __name__ == "__main__":
    main()
