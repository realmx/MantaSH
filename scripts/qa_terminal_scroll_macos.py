#!/usr/bin/env python3
"""Verify real npm/Vite output, geometry and scrollbar state in an isolated macOS window.

Supply an already installed Vite CLI (tested with 8.3.0). No package installation,
user rc files, existing sessions or application data are touched.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shlex
import subprocess

from qa_overview_layout_macos import Driver


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--vite-cli", type=Path, required=True)
    args = parser.parse_args()
    cli = args.vite_cli.resolve(strict=True)
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=False)
    (root / "package.json").write_text(json.dumps({
        "name": "mantash-scroll-fixture", "version": "1.0.0",
        "scripts": {"dev": "node " + shlex.quote(str(cli)) + " --host 127.0.0.1 --port 0",
                    "overflow": "node overflow.cjs"},
    }))
    (root / "overflow.cjs").write_text(
        "for (let i=0;i<process.stdout.rows+10;i++) console.log('QA_LINE_'+i);\n"
        "console.log('QA_OVERFLOW_READY');\n"
        "process.stdin.on('data',()=>{process.stdout.write('\\x1b[3J\\x1b[H\\x1b[2J');"
        "console.log('QA_CLEARED_RUNNING');});\n"
        "setTimeout(()=>process.exit(0),600000);\n")
    rc = root / "zsh"
    rc.mkdir()
    (rc / ".zshrc").write_text("HISTFILE=/dev/null\nPROMPT='$ '\nRPROMPT='[develop][qa123456]'\n")
    env = {**os.environ, "SHELL": "/bin/zsh", "ZDOTDIR": str(rc),
           "MANTASH_DATA_DIR": str(root / "data"), "MANTASH_QA_CONTROL": str(root / "command.json")}
    env.pop("MANTASH_QA_BACKGROUND", None)
    # Vite enables its terminal refresh only for interactive, non-CI output.
    env.pop("CI", None)
    results = []
    with (root / "stdout.log").open("w") as stdout, (root / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=root, env=env,
                                   stdout=stdout, stderr=stderr)
        driver = Driver(root, process)
        def tab(state):
            return state["tabs"][state["active_tab"]]
        def terminal(state):
            return tab(state)["panes"][tab(state)["active_pane"]]["terminal"]
        def fits(state):
            t = terminal(state)
            g = t["geometry"]
            return (t["rows"] == math.floor(g["height"] / g["line_height"])
                    and t["history_lines"] == 0 and t["scroll_metrics"] is None
                    and g["rendered_scroll_metrics"] is None and tab(state)["viewport_max_y"] == 0)
        def record(label, state):
            t = terminal(state)
            results.append({"case": label, "rows": t["rows"], "geometry": t["geometry"],
                            "history_lines": t["history_lines"], "scroll_metrics": t["scroll_metrics"],
                            "display_offset": t["display_offset"], "viewport_max_y": tab(state)["viewport_max_y"]})
        try:
            driver.wait(lambda s: bool(s["tabs"]) and terminal(s) and terminal(s)["command_editing"], "zsh prompt")
            driver.action("resize", width=1030, height=1288)
            driver.wait(fits, "settled empty viewport")
            driver.action("type", text="cd " + shlex.quote(str(root)) + " && npm run dev\r")
            state = driver.wait(lambda s: "Local:" in terminal(s)["text"] and fits(s), "real Vite startup without phantom scrollbar")
            assert "npm run dev" in terminal(state)["text"], terminal(state)
            assert "[develop][qa123456]" in terminal(state)["text"], terminal(state)
            record("vite_startup", state)
            for width, height in [(1440, 900), (1030, 1288)]:
                driver.action("resize", width=width, height=height)
                state = driver.wait(lambda s: s["width"] == width and s["height"] == height
                                    and fits(s) and "Local:" in terminal(s)["text"], "Vite after resize")
                record("vite_resize", state)
            driver.action("local")
            driver.wait(lambda s: len(s["tabs"]) == 2, "second tab")
            driver.action("tab", index=0)
            state = driver.wait(lambda s: s["active_tab"] == 0 and fits(s), "return to Vite tab")
            record("vite_tab_return", state)
            driver.action("keystroke", key="ctrl-c")
            driver.wait(lambda s: terminal(s)["command_editing"], "Vite stopped")
            driver.action("type", text="npm run overflow\r")
            state = driver.wait(lambda s: "QA_OVERFLOW_READY" in terminal(s)["text"]
                                and terminal(s)["scroll_metrics"] is not None
                                and terminal(s)["geometry"]["rendered_scroll_metrics"] is not None, "real overflow scrollbar")
            record("overflow", state)
            driver.action("scroll_terminal", lines=10000)
            state = driver.wait(lambda s: terminal(s)["display_offset"] > 0
                                and "QA_LINE_0\n" in terminal(s)["text"], "scroll to earlier output")
            record("scroll_up", state)
            driver.action("scroll_terminal", lines=-10000)
            state = driver.wait(lambda s: terminal(s)["display_offset"] == 0
                                and "QA_OVERFLOW_READY" in terminal(s)["text"], "scroll to live bottom")
            record("scroll_bottom", state)
            driver.action("type", text="clear\r")
            state = driver.wait(lambda s: "QA_CLEARED_RUNNING" in terminal(s)["text"] and fits(s), "running process clears history and scrollbar")
            record("clear_running", state)
            driver.action("keystroke", key="ctrl-c")
            driver.wait(lambda s: terminal(s)["command_editing"], "fixture stopped")
        finally:
            if process.poll() is None:
                driver.send("quit")
            process.wait(timeout=20)
    (root / "terminal-scroll-results.json").write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"report": str(root / "terminal-scroll-results.json"), "cases": len(results),
                      "scope": "real npm/Vite, zsh RPROMPT, native layout and programmatic scrolling; no physical mouse test"}))


if __name__ == "__main__":
    main()
