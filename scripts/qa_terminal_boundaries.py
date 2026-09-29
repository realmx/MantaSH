#!/usr/bin/env python3
"""Verify real PTY boundaries and Vite-style refreshes in an isolated native window.

Requires Node.js. On Windows, pass --shell with the installed Git Bash executable.
"""
import argparse
import json
import math
import os
from pathlib import Path
import platform
import shlex
import subprocess

from qa_overview_layout_macos import Driver


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--shell", help="Shell executable; defaults to /bin/zsh on macOS")
    args = parser.parse_args()
    if platform.system() not in ("Darwin", "Windows"):
        parser.error("This native acceptance driver supports macOS and Windows")
    if os.name == "nt" and not args.shell:
        parser.error("Windows requires --shell pointing to Git Bash")
    shell = args.shell or "/bin/zsh"
    if Path(shell).stem.lower() not in ("bash", "zsh"):
        parser.error("Use Bash or Zsh; fixture command quoting requires a POSIX shell")
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=False)
    fixture = root / "boundaries.cjs"
    fixture.write_text(
        "process.stdin.setRawMode(true);\n"
        "process.stdin.resume();\n"
        "function render(key) {\n"
        " const rows=process.stdout.rows;\n"
        " if(key==='v') {\n"
        "  process.stdout.write('\\r\\n'.repeat(rows-1)+'\\x1b[1;1H\\x1b[0JQA_v_READY');\n"
        "  return;\n"
        " }\n"
        " const count=key==='e'?rows:key==='o'?rows+10:3;\n"
        " const lines=Array.from({length:count},(_,i)=>'QA_'+key+'_'+i);\n"
        " process.stdout.write('\\x1b[3J\\x1b[H\\x1b[2J'+lines.join('\\r\\n'));\n"
        "}\n"
        "process.stdin.on('data',data=>render(data.toString().trim()));\n"
        "render('s');\n"
    )
    rc = root / "zsh"
    rc.mkdir()
    (rc / ".zshrc").write_text("HISTFILE=/dev/null\nPROMPT='$ '\nRPROMPT=''\n")
    env = {**os.environ, "ZDOTDIR": str(rc),
           "MANTASH_DATA_DIR": str(root / "data"),
           "MANTASH_QA_CONTROL": str(root / "command.json"),
           "MANTASH_QA_BACKGROUND": "1"}
    results = []
    with (root / "stdout.log").open("w") as stdout, (root / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=root, env=env,
                                   stdout=stdout, stderr=stderr)
        driver = Driver(root, process)

        def terminal(state):
            tab = state["tabs"][state["active_tab"]]
            return tab["panes"][tab["active_pane"]]["terminal"]

        def fits(state):
            term = terminal(state)
            geometry = term["geometry"]
            tab = state["tabs"][state["active_tab"]]
            return (term["rows"] == math.floor(geometry["height"] / geometry["line_height"])
                    and term["history_lines"] == 0 and term["scroll_metrics"] is None
                    and geometry["rendered_scroll_metrics"] is None and tab["viewport_max_y"] == 0)

        def record(case, state):
            term = terminal(state)
            results.append({"case": case, "rows": term["rows"],
                            "history_lines": term["history_lines"],
                            "display_offset": term["display_offset"],
                            "scroll_metrics": term["scroll_metrics"],
                            "geometry": term["geometry"]})

        try:
            driver.wait(lambda s: bool(s["tabs"]) and terminal(s), "startup")
            driver.action("local", shell=shell)
            driver.wait(lambda s: len(s["tabs"]) == 2 and s["active_tab"] == 1
                        and terminal(s)["command_editing"], "selected shell prompt")
            driver.action("type", text="node " + shlex.quote(fixture.as_posix()) + "\r")
            driver.wait(lambda s: "QA_s_0" in terminal(s)["text"] and fits(s), "PTY fixture ready")
            for width, height in ((1280, 800), (960, 640)):
                driver.action("resize", width=width, height=height)
                driver.wait(lambda s: s["width"] == width and s["height"] == height and fits(s), "settled resize")
                for key, label in (("s", "short"), ("e", "exact"), ("o", "overflow")):
                    driver.action("type", text=key)
                    if key != "o":
                        state = driver.wait(lambda s: f"QA_{key}_0" in terminal(s)["text"] and fits(s), label)
                        count = terminal(state)["rows"] if key == "e" else 3
                        assert f"QA_{key}_{count - 1}" in terminal(state)["text"]
                    else:
                        state = driver.wait(lambda s: terminal(s)["history_lines"] == 10
                                            and terminal(s)["scroll_metrics"] is not None
                                            and terminal(s)["geometry"]["rendered_scroll_metrics"] is not None,
                                            "overflow scrollbar")
                        count = terminal(state)["rows"] + 10
                        assert f"QA_o_{count - 1}" in terminal(state)["text"]
                    record(f"{width}x{height}_{label}", state)
                driver.action("scroll_terminal", lines=10000)
                top = driver.wait(lambda s: terminal(s)["display_offset"] == 10
                                  and "QA_o_0\n" in terminal(s)["text"], "first overflow line")
                record("scroll_top", top)
                driver.action("scroll_terminal", lines=-10000)
                bottom = driver.wait(lambda s: terminal(s)["display_offset"] == 0
                                     and f"QA_o_{count - 1}" in terminal(s)["text"], "last overflow line")
                record("scroll_bottom", bottom)
                driver.action("type", text="s")
                cleared = driver.wait(lambda s: "QA_s_0" in terminal(s)["text"] and fits(s), "clear history")
                record("clear_hides_scrollbar", cleared)
                driver.action("type", text="v")
                refreshed = driver.wait(lambda s: "QA_v_READY" in terminal(s)["text"] and fits(s), "Vite-style refresh")
                assert "QA_s_0" in terminal(refreshed)["text"]
                record("vite_refresh_preserves_short_output", refreshed)
                driver.action("type", text="s")
                driver.wait(lambda s: "QA_s_0" in terminal(s)["text"] and "QA_v_READY" not in terminal(s)["text"]
                            and fits(s), "reset after refresh")
        finally:
            if process.poll() is None:
                driver.send("quit")
                try:
                    process.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=10)
    assert process.returncode == 0, process.returncode
    report = root / "terminal-boundary-results.json"
    report.write_text(json.dumps({"platform": platform.platform(), "shell": shell,
                                  "checks": results}, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"cases": len(results), "report": str(report), "platform": platform.system(),
                      "shell": shell, "scope": "real PTY, native layout and programmatic scrolling; no physical pointer validation"}))


if __name__ == "__main__":
    main()
