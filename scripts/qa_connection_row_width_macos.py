#!/usr/bin/env python3
"""Assert connection library rows span the full list width and the footer keeps no per-row actions."""
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
    """Drive the isolated debug app through the real connection library rendering path."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
                   "MANTASH_QA_CONTROL": str(directory / "command.json"), "MANTASH_QA_BACKGROUND": "1"}
    connections = [(f"loopback fixture {index}", "127.0.0.1", 22, "fixture") for index in range(12)]
    process = subprocess.Popen([str(args.binary.resolve())], cwd=directory, env=environment,
                               stdout=(directory / "stdout.log").open("w"),
                               stderr=(directory / "stderr.log").open("w"))
    driver = Driver(directory, process)
    checks = []
    try:
        driver.wait(lambda state: len(state["tabs"]) == 1, "startup")
        # Pin the window so dialog geometry asserts against known values.
        driver.action("resize", width=1280, height=800)
        import_text = "name,host,port,username,password\r\n" + "".join(
            f'"{name}",{host},{port},{user},\r\n' for name, host, port, user in connections)
        driver.action("import", text=import_text)
        driver.action("confirm_import")
        driver.action("sidebar")
        library = driver.wait(
            lambda state: state["modal"] == "connections" and state["connection_library"]["rows"],
            "library rows painted")
        rows = library["connection_library"]["rows"]
        assert rows and len(rows) >= 8, f"expected painted rows, got {len(rows)}"
        bounds = library["connection_library"]["bounds"]
        for row in rows:
            assert abs(row["bounds"]["width"] - bounds["width"]) < 0.5, \
                f"row {row['id']} width {row['bounds']['width']} != list width {bounds['width']}"
        # The measurement canvas sits at its static position inside the row's
        # left padding, so rows share one x instead of matching the list x.
        assert len({row["bounds"]["x"] for row in rows}) == 1, "rows must share one left edge"
        checks.append(f"{len(rows)} rows span the full {bounds['width']:g}px list width")
        # 640px dialog (1px borders) inset 12px each side: 638 - 24 = 614.
        assert abs(bounds["width"] - 614.) < 0.5, f"list width {bounds['width']} != 614"
        assert abs(bounds["x"] - 333.) < 0.5, f"list x {bounds['x']} != 333 (12px side insets)"
        checks.append("list keeps 12px side insets inside the 640px dialog")
        # Dialog height caps at 600px: 800px viewport would otherwise give a
        # ~621px list (752px dialog minus chrome); with the cap it is ~470.
        assert bounds["height"] < 500., f"list height {bounds['height']} exceeds the 600px dialog cap"
        checks.append(f"dialog capped at 600px (list height {bounds['height']:g}px)")
        driver.action("connection_query", query="loopback fixture 9")
        filtered = driver.wait(
            lambda state: len(state["connection_library"]["visible"]) == 1 and state["connection_library"]["rows"],
            "filtered rows painted")
        for row in filtered["connection_library"]["rows"]:
            assert abs(row["bounds"]["width"] - filtered["connection_library"]["bounds"]["width"]) < 0.5
        checks.append("filtered rows keep the full list width")
    finally:
        if process.poll() is None:
            driver.send("quit")
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
    (directory / "row-width-results.json").write_text(json.dumps(
        {"checks": checks, "footer": "batch delete, clear selection, import, export only"}, indent=2) + "\n")
    print(json.dumps({"checks": checks, "report": str(directory / "row-width-results.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
