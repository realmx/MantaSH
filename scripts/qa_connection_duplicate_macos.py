#!/usr/bin/env python3
"""Assert connection library opens always create fresh tabs, allowing duplicate sessions."""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from qa_overview_layout_macos import Driver


def main():
    """Drive the real library Enter path twice against one refused loopback endpoint."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
                   "MANTASH_QA_CONTROL": str(directory / "command.json"), "MANTASH_QA_BACKGROUND": "1"}
    # Nothing listens on this loopback port: the tab appears immediately and the
    # session fails on its own, so no real SSH server or credential is touched.
    connections = [("duplicate fixture", "127.0.0.1", 39391, "fixture")]
    process = subprocess.Popen([str(args.binary.resolve())], cwd=directory, env=environment,
                               stdout=(directory / "stdout.log").open("w"),
                               stderr=(directory / "stderr.log").open("w"))
    driver = Driver(directory, process)
    checks = []
    try:
        driver.wait(lambda state: len(state["tabs"]) == 1, "startup")
        import_text = "name,host,port,username,password\r\n" + "".join(
            f'"{name}",{host},{port},{user},\r\n' for name, host, port, user in connections)
        driver.action("import", text=import_text)
        driver.action("confirm_import")
        for round_index in range(2):
            driver.action("sidebar")
            # Enter on the focused search field routes through the same public
            # open path as the row's connect button.
            driver.action("keystroke", key="enter")
            driver.wait(lambda state: len(state["tabs"]) == 2 + round_index,
                        f"open #{round_index + 1} creates a fresh tab")
        final = driver.action("snapshot")
        labels = [tab["panes"][0]["label"] for tab in final["tabs"]]
        assert labels.count("duplicate fixture") == 2, labels
        checks.append("two opens of one connection keep two separate tabs")
    finally:
        if process.poll() is None:
            driver.send("quit")
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
    (directory / "duplicate-results.json").write_text(json.dumps(
        {"checks": checks, "scope": "library Enter path against a refused loopback endpoint"}, indent=2) + "\n")
    print(json.dumps({"checks": checks, "report": str(directory / "duplicate-results.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
