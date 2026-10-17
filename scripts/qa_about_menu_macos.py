#!/usr/bin/env python3
"""Verify the macOS app menu opens the shared About dialog in an isolated native window."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from qa_overview_layout_macos import Driver


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=False)
    project = Path(__file__).resolve().parents[1]
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=project
    ))
    version = next(package["version"] for package in metadata["packages"] if package["name"] == "mantash")
    env = {**os.environ, "MANTASH_DATA_DIR": str(root / "data"),
           "MANTASH_QA_CONTROL": str(root / "command.json")}
    # The active-window dispatch path must not inherit hidden/background QA flags.
    env.pop("MANTASH_QA_BACKGROUND", None)
    env.pop("MANTASH_QA_HIDDEN", None)
    checks = []
    with (root / "stdout.log").open("w") as stdout, (root / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=root, env=env,
                                   stdout=stdout, stderr=stderr)
        driver = Driver(root, process)
        try:
            driver.wait(lambda state: state.get("application_menu", {}).get("registered")
                        and state["active_window"] and bool(state["tabs"]),
                        "native application menu")
            for english, label, settings in [
                (False, "关于 MantaSH", "设置"),
                (True, "About MantaSH", "Settings"),
            ]:
                driver.action("language", english=english)
                state = driver.wait(lambda state: state["application_menu"]["items"][0] == label,
                                    "localized application menu")
                items = state["application_menu"]["items"]
                assert items[:4] == [label, "", settings, ""], items
                assert "Services" in items and items[-1] in ("退出 MantaSH", "Quit MantaSH"), items
                assert state["modal"] is None
                assert state["active_window"], "About must be routed through an active window"
                driver.action("application_about_menu")
                state = driver.wait(lambda state: state["modal"] == "about",
                                    "Application menu opens the shared About dialog")
                state = driver.draw_dialog()
                frame = state["dialog_frame_bounds"]
                assert frame and frame["width"] <= 640 and state["modal_scroll"]["max_x"] == 0
                assert frame["width"] > 0 and frame["height"] > 0
                assert frame["x"] >= 0 and frame["y"] >= 0
                assert frame["x"] + frame["width"] <= state["width"]
                assert frame["y"] + frame["height"] <= state["height"]
                driver.action("application_about_menu")
                repeated = driver.draw_dialog()
                assert repeated["modal"] == "about" and repeated["modal_stack"] == []
                checks.append({"language": "en" if english else "zh", "menu": items,
                               "source_version": version, "frame": frame})
                driver.action("keystroke", key="escape")
                driver.wait(lambda state: state["modal"] is None, "About closes")

            driver.action("settings")
            driver.wait(lambda state: state["modal"] == "settings", "Settings opens")
            driver.draw_dialog()
            driver.action("application_about_menu")
            driver.wait(lambda state: state["modal"] == "about" and "settings" in state["modal_stack"],
                        "Shared About retains the Settings parent")
            driver.draw_dialog()
            driver.action("application_about_menu")
            repeated = driver.draw_dialog()
            assert repeated["modal"] == "about" and repeated["modal_stack"] == ["settings"]
            driver.action("keystroke", key="escape")
            driver.wait(lambda state: state["modal"] == "settings", "Closing About returns to Settings")
            driver.action("keystroke", key="escape")
            driver.wait(lambda state: state["modal"] is None, "Settings closes")
        finally:
            if process.poll() is None:
                driver.send("quit")
            assert process.wait(timeout=20) == 0
    result = {"binary": str(args.binary.resolve()), "version": version, "checks": checks,
              "settings_preserved": True, "repeated_open_preserves_parent": True,
              "scope": "isolated active-window AppKit menu invocation and shared GPUI About dialog; no physical menu click or Windows runtime test"}
    (root / "about-menu-results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"report": str(root / "about-menu-results.json"), "cases": len(checks)}))


if __name__ == "__main__":
    main()
