#!/usr/bin/env python3
"""Verify the real macOS app menu and system About panel in an isolated native window."""
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
    checks = []
    with (root / "stdout.log").open("w") as stdout, (root / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=root, env=env,
                                   stdout=stdout, stderr=stderr)
        driver = Driver(root, process)
        try:
            driver.wait(lambda state: state.get("application_menu", {}).get("registered") and bool(state["tabs"]),
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
                assert state["modal"] is None and not state["about_panel"]["visible"], state
                driver.action("application_about_menu")
                state = driver.wait(lambda state: state["about_panel"]["visible"]
                                    and state["about_panel"]["version_visible"]
                                    and state["modal"] is None,
                                    "native About panel opens outside the workbench")
                assert state["notice"] is None, state["notice"]
                checks.append({"language": "en" if english else "zh", "menu": items,
                               "observed_version": version, "panel": state["about_panel"]})
                driver.action("application_about_close")
                driver.wait(lambda state: not state["about_panel"]["visible"]
                            and state["modal"] is None, "native About panel closes")

            driver.action("settings")
            driver.wait(lambda state: state["modal"] == "settings", "Settings opens")
            driver.action("application_about_menu")
            driver.wait(lambda state: state["about_panel"]["visible"]
                        and state["modal"] == "settings" and state["modal_stack"] == [],
                        "native About does not replace Settings")
            driver.action("application_about_close")
            driver.wait(lambda state: not state["about_panel"]["visible"]
                        and state["modal"] == "settings", "Settings survives closing About")
            driver.action("keystroke", key="escape")
            driver.wait(lambda state: state["modal"] is None, "Settings closes")
        finally:
            if process.poll() is None:
                driver.send("quit")
            assert process.wait(timeout=20) == 0
    result = {"binary": str(args.binary.resolve()), "version": version, "checks": checks,
              "settings_preserved": True, "scope": "isolated AppKit menu and standard About panel; no physical menu click or Windows runtime test"}
    (root / "about-menu-results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"report": str(root / "about-menu-results.json"), "cases": len(checks)}))


if __name__ == "__main__":
    main()
