#!/usr/bin/env python3
"""Exercise history/connection modal selection, Escape, scroll and footer geometry."""
import argparse
import json
import os
import subprocess
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from qa_overview_layout_macos import Driver


def history_entries():
    """Return two SSH origins and enough rows for range selection."""
    origins = [str(uuid.uuid4()), str(uuid.uuid4())]
    return [
        {"id": str(uuid.uuid4()), "scope": f"ssh:{origins[index % 2]}",
         "command": f"history fixture {index}", "timestamp": index}
        for index in range(100)
    ]


def main():
    """Run production modal actions against one isolated debug process."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {
        **os.environ,
        "MANTASH_DATA_DIR": str(directory / "data"),
        "MANTASH_QA_CONTROL": str(directory / "command.json"),
        "MANTASH_QA_BACKGROUND": "1",
    }
    process = subprocess.Popen(
        [str(args.binary.resolve())],
        cwd=directory,
        env=environment,
        stdout=(directory / "stdout.log").open("w"),
        stderr=(directory / "stderr.log").open("w"),
    )
    driver = Driver(directory, process)
    checks = []
    try:
        driver.wait(lambda state: len(state["tabs"]) == 1, "startup")

        driver.action("history_fixture", entries=history_entries())
        driver.action("draw")
        history = driver.wait(
            lambda state: state["modal"] == "history"
            and state["history_view"]["rows"]
            and state["history_view"]["modal_bounds"]
            and state["history_view"]["adaptive_height"]
            and state["history_view"]["adaptive_height"] > 320,
            "history modal rows",
        )
        driver.action("draw")
        history = driver.action("snapshot")
        assert 320 < history["history_view"]["adaptive_height"] <= 600
        checks.append("history modal height grows with content within the 320-600px range")
        driver.action("history_query", query="history fixture 11")
        driver.action("draw")
        driver.action("draw")
        short = driver.wait(
            lambda state: state["modal"] == "history"
            and len(state["history_view"]["rows"]) == 1
            and state["history_view"]["adaptive_height"] < 600
            and state["history_view"]["max_y"] <= 2,
            "short history has no overflow",
        )
        checks.append("short history fits without a scrollbar")
        driver.action("history_query", query="history fixture 1")
        driver.action("draw")
        driver.action("draw")
        medium = driver.wait(
            lambda state: state["modal"] == "history"
            and len(state["history_view"]["rows"]) == 11
            and 320 < state["history_view"]["adaptive_height"] < 600
            and state["history_view"]["max_y"] <= 2,
            "medium history has no overflow",
        )
        checks.append("adaptive history under 600px has no scrollbar")
        driver.action("history_select", index=0)
        driver.action("draw")
        driver.action("draw")
        selected_medium = driver.wait(
            lambda state: state["modal"] == "history"
            and len(state["history_view"]["selected"]) == 1
            and state["history_view"]["adaptive_height"] < 600
            and state["history_view"]["max_y"] <= 2,
            "history footer fits without overflow",
        )
        checks.append("selected history grows to fit its footer without a scrollbar")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] == "history"
                    and not state["history_view"]["selected"],
                    "history selection cleared before query reset")
        driver.action("history_query", query="")
        driver.action("draw")
        driver.action("draw")
        history = driver.wait(
            lambda state: state["modal"] == "history"
            and len(state["history_view"]["rows"]) == 100
            and state["history_view"]["adaptive_height"] == 600
            and state["history_view"]["max_y"] > 0,
            "capped history scrolls",
        )
        assert history["history_view"]["max_y"] > 0
        bounds = history["history_view"]["bounds"]
        x = bounds["x"] + bounds["width"] - 5
        start_y = bounds["y"] + 8
        end_y = bounds["y"] + bounds["height"] - 8
        driver.action("pointer_gesture", points=[[x, start_y], [x, start_y], [x, end_y], [x, end_y]])
        scrolled = driver.wait(
            lambda state: state["history_view"]["scroll_y"] < -20,
            "history scrollbar pointer drag",
        )
        checks.append("history scrollbar pointer drag changes the real scroll offset")
        driver.action("history_select", index=1)
        driver.action("history_select", index=4, shift=True)
        selected = driver.action("snapshot")["history_view"]["selected"]
        assert len(selected) == 4, selected
        checks.append("history Shift selection uses the visible UUID order")
        driver.action("history_select", index=2, additive=True)
        selected = driver.action("snapshot")["history_view"]["selected"]
        assert len(selected) == 3, selected
        checks.append("history additive selection toggles one visible row")
        driver.action("keystroke", key="escape")
        cleared = driver.wait(
            lambda state: state["modal"] == "history" and not state["history_view"]["selected"],
            "history Escape clears selection",
        )
        assert cleared["history_view"]["anchor"] is None
        checks.append("history Escape clears selection and keeps the modal open")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "history Escape closes empty modal")
        checks.append("history Escape closes when no row is selected")

        for ui in (14, 18):
            driver.action("font_sizes", ui=ui, terminal=12)
            driver.action("theme", night=ui == 18)
            for download in (False, True):
                driver.action("transfer_fixture", confirm_upload=True, download=download)
                driver.action("draw")
                driver.action("draw")
                state = driver.wait(
                    lambda state: state["modal"] == "transfer"
                    and state["transfer_view"] and state["transfer_view"]["phase"] == "review"
                    and state["transfer_view"]["footer_bounds"]
                    and state["modal_scroll"]["content_bounds"],
                    f"{ui}px {'download' if download else 'upload'} review layout",
                )
                endpoints = state["transfer_view"]["endpoints"]
                assert len(endpoints) == len(state["pending_transfers"]) == 3
                for endpoint, record in zip(endpoints, state["pending_transfers"]):
                    assert endpoint["source"] == (record["remote"] if download else record["local"])
                    assert endpoint["target"] == (record["local"] if download else record["remote"])
                    assert endpoint["name"] == endpoint["source"].rsplit("/", 1)[-1]
                frame = state["dialog_frame_bounds"]
                viewport = state["modal_scroll"]["bounds"]
                content = state["modal_scroll"]["content_bounds"]
                footer = state["transfer_view"]["footer_bounds"]
                assert abs(frame["width"] + 2 - 480) < 1, frame
                assert abs(content["x"] - viewport["x"] - 12) < 1
                assert abs(viewport["x"] + viewport["width"] - content["x"] - content["width"] - 12) < 1
                assert footer["y"] >= viewport["y"] + viewport["height"] - 1
                assert state["modal_scroll"]["max_x"] <= 1
                geometry = state["transfer_view"]["geometry"]
                assert geometry["source_path"]["width"] > 100, geometry
                assert geometry["row"]["height"] < 180, geometry
                assert state["modal_scroll"]["max_y"] < 100, state["modal_scroll"]
                baseline = content["height"]
                checks.append(f"{ui}px {'download' if download else 'upload'} review keeps fixed footer and endpoint columns")

                driver.action("transfer_fixture", confirm_upload=True, download=download, long_path=True,
                              unicode=download)
                driver.action("draw")
                driver.action("draw")
                long_view = driver.wait(
                    lambda state: state["modal"] == "transfer"
                    and state["transfer_view"] and state["transfer_view"]["endpoints"]
                    and state["modal_scroll"]["content_bounds"]
                    and state["modal_scroll"]["content_bounds"]["height"] > baseline + 24
                    and state["modal_scroll"]["max_x"] <= 1,
                    f"{ui}px long {'download' if download else 'upload'} paths wrap",
                )
                if download:
                    assert "中文长文件名" in long_view["transfer_view"]["endpoints"][0]["name"]
                checks.append(f"{ui}px {'download' if download else 'upload'} long paths wrap inside list")

        driver.action("font_sizes", ui=14, terminal=12)
        driver.action("theme", night=False)
        driver.action("resize", width=960, height=640)
        driver.wait(lambda state: state["width"] == 960 and state["height"] == 640, "minimum modal window")
        driver.action("transfer_fixture", confirm_upload=True, download=True, long_path=True,
                      unicode=True, count=40)
        driver.action("draw")
        driver.action("draw")
        dense = driver.wait(
            lambda state: state["modal"] == "transfer" and state["transfer_view"]
            and state["transfer_view"]["footer_bounds"]
            and state["modal_scroll"]["max_y"] > 100
            and state["modal_scroll"]["max_x"] <= 1,
            "minimum-window forty-row transfer review scrolls inside the list",
        )
        assert dense["dialog_frame_bounds"]["width"] <= 960 - 32
        footer_before = dense["transfer_view"]["footer_bounds"]
        driver.action("scroll_modal", y=-100000)
        driver.action("draw")
        scrolled = driver.wait(lambda state: state["modal"] == "transfer"
                               and state["modal_scroll_y"] < -100
                               and state["transfer_view"]["footer_bounds"],
                               "transfer list scroll with fixed footer")
        assert abs(scrolled["transfer_view"]["footer_bounds"]["y"] - footer_before["y"]) <= 1
        checks.append("minimum-window long-path list scrolls with a fixed footer and no horizontal overflow")
        driver.action("resize", width=1280, height=800)
        driver.wait(lambda state: state["width"] == 1280 and state["height"] == 800, "restore modal window")

        for progress in (None, 0, 25, 75, 100):
            driver.action("transfer_fixture", confirm_upload=True, active=True, progress=progress)
            driver.action("draw")
            running = driver.wait(lambda state: state["modal"] == "transfer"
                                  and state["transfer_view"] and state["transfer_view"]["phase"] == "running"
                                  and state["transfer_view"]["footer_bounds"]
                                  and state["transfers"][0]["total"] == (100 if progress is not None else None),
                                  f"running transfer {progress}")
            assert running["modal_scroll"]["max_x"] <= 1
        checks.append("running transfer renders unknown and 0/25/75/100 determinate status")
        fixed = driver.action("review_stop_transfers")
        stopped = driver.wait(lambda state: state["modal"] == "cancel_transfers"
                              and state["transfer_stop_targets"], "stop confirmation")
        profile = running["pending_transfers"][0]["profile"]
        assert stopped["transfer_stop_targets"]["host"] == f"{profile['host']}:{profile['port']}"
        assert stopped["transfer_stop_targets"]["ids"] == [running["transfers"][0]["id"]]
        checks.append("stop confirmation fixes host and unfinished UUIDs")
        driver.action("keystroke", key="escape")
        resumed = driver.wait(lambda state: state["modal"] == "transfer"
                              and state["transfer_view"] and state["transfer_view"]["phase"] == "running",
                              "stop cancelled to running batch")
        assert resumed["transfer_view"]["batch"] == running["transfer_view"]["batch"]
        driver.action("transfer_background")
        driver.wait(lambda state: state["modal"] is None, "background view closed")
        checks.append("closing the running view does not stop its batch")

        driver.action("transfer_fixture", confirm_upload=True, failed=True)
        driver.action("draw")
        result = driver.wait(lambda state: state["modal"] == "transfer"
                             and state["transfer_view"] and state["transfer_view"]["phase"] == "result"
                             and state["transfers"][0]["state"] == "failed"
                             and state["transfers"][0]["error"], "failed batch result")
        assert result["transfer_view"]["footer_bounds"]
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "result closed without deletion")
        checks.append("failed batch stays in results until explicitly closed")
        for ui in (14, 18):
            driver.action("font_sizes", ui=ui, terminal=12)
            driver.action("transfer_fixture", confirm_upload=False)
            driver.action("draw")
            driver.action("draw")
            baseline = driver.wait(
                lambda state: state["modal"] == "transfers"
                and state["preferences"]["ui_size"] == ui
                and state["modal_scroll"]["content_bounds"],
                f"{ui}px transfer history baseline",
            )["modal_scroll"]["content_bounds"]
            driver.action("keystroke", key="escape")
            driver.wait(lambda state: state["modal"] is None, f"{ui}px baseline closed")
            driver.action("transfer_fixture", confirm_upload=False, long_path=True)
            driver.action("draw")
            driver.action("draw")
            long_history = driver.wait(
                lambda state: state["modal"] == "transfers"
                and state["transfers"][0]["remote"].endswith("longfilename" * 32 + ".txt")
                and state["modal_scroll"]["content_bounds"]
                and state["modal_scroll"]["content_bounds"]["height"] > baseline["height"] + 24
                and state["modal_scroll"]["max_x"] <= 1,
                f"{ui}px transfer history filename wraps",
            )
            assert long_history["modal_scroll"]["content_bounds"]["width"] <= baseline["width"] + 1
            checks.append(f"{ui}px transfer history wraps long filenames without horizontal overflow")
            driver.action("keystroke", key="escape")
            driver.wait(lambda state: state["modal"] is None, f"{ui}px long history closed")
        driver.action("font_sizes", ui=14, terminal=12)
        driver.action("transfer_fixture", confirm_upload=False)
        driver.wait(lambda state: state["modal"] == "transfers", "transfer history modal")
        driver.action("transfer_select", index=1)
        driver.wait(lambda state: len(state["transfer_selection"]["selected"]) == 1,
                    "transfer row selected")
        driver.action("keystroke", key="escape")
        cleared = driver.wait(lambda state: state["modal"] == "transfers"
                              and not state["transfer_selection"]["selected"],
                              "transfer Escape clears selection")
        assert cleared["transfer_selection"]["anchor"] is None
        checks.append("transfer Escape clears selection and keeps dialog open")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None,
                    "transfer Escape closes unselected dialog")
        checks.append("transfer Escape closes when no record is selected")
        driver.action("transfer_fixture", confirm_upload=False, active=True)
        seeded = driver.wait(lambda state: state["modal"] == "transfers"
                             and len(state["transfers"]) == 3,
                             "transfer removal fixture")
        active_id = seeded["transfers"][0]["id"]
        finished_id = seeded["transfers"][-1]["id"]
        driver.action("transfer_select", index=0)
        driver.action("remove_transfer_records")
        removed = driver.wait(lambda state: state["modal"] == "transfers"
                              and len(state["transfers"]) == 2
                              and not state["transfer_selection"]["selected"],
                              "selected transfer removed directly")
        assert finished_id not in {task["id"] for task in removed["transfers"]}
        assert active_id in {task["id"] for task in removed["transfers"]}
        checks.append("selected finished record removes immediately without another modal")
        driver.action("remove_transfer_records")
        remaining = driver.wait(lambda state: state["modal"] == "transfers"
                                and len(state["transfers"]) == 1,
                                "all finished transfers removed directly")
        assert remaining["transfers"][0]["id"] == active_id
        checks.append("unselected Remove all keeps running transfers")
        driver.action("transfer_select", index=0)
        driver.action("remove_transfer_records")
        blocked = driver.action("snapshot")
        assert blocked["modal"] == "transfers" and len(blocked["transfers"]) == 1
        assert blocked["transfer_selection"]["selected"] == [active_id]
        checks.append("a selected running transfer cannot be removed")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] == "transfers"
                    and not state["transfer_selection"]["selected"],
                    "running transfer selection cleared")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "transfer dialog closed after removal")
        driver.action("transfer_fixture", confirm_upload=False)
        driver.wait(lambda state: state["modal"] == "transfers"
                    and all(task["state"] == "completed" for task in state["transfers"]),
                    "fixture resets active state")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "fixture leaves no active task")
        profiles = [(f"modal fixture {index}", "127.0.0.1", 22, "fixture") for index in range(24)]
        csv = "name,host,port,username,password\r\n" + "".join(
            f'"{name}",{host},{port},{user},\r\n' for name, host, port, user in profiles
        )
        driver.action("import", text=csv)
        driver.action("confirm_import")
        driver.action("sidebar")
        driver.action("draw")
        driver.action("draw")
        library = driver.wait(
            lambda state: state["modal"] == "connections"
            and len(state["connection_library"]["visible"]) == len(profiles) + 2
            and state["connection_library"]["scrollbar_bounds"]
            and state["connection_library"]["add_bounds"],
            "connection modal geometry",
        )
        geometry = library["connection_library"]
        scrollbar = geometry["scrollbar_bounds"]
        thumb = geometry["thumb_bounds"]
        bounds = geometry["bounds"]
        assert abs((scrollbar["x"] + scrollbar["width"]) - (bounds["x"] + bounds["width"]) - 12) < 1
        checks.append("connection scrollbar is painted flush with the list right edge")
        assert geometry["add_bounds"]["width"] > 0
        checks.append("connection footer Add button is painted and measurable")

        driver.action("connection_select", index=1)
        driver.action("connection_select", index=4, shift=True)
        selected = driver.action("snapshot")["connection_library"]["multi"]
        assert len(selected) == 4, selected
        driver.action("keystroke", key="escape")
        cleared = driver.wait(
            lambda state: state["modal"] == "connections"
            and not state["connection_library"]["multi"]
            and state["connection_library"]["highlighted"] is None,
            "connection Escape clears selection",
        )
        assert cleared["connection_library"]["anchor"] is None
        checks.append("connection Escape clears selection and keeps the modal open")

        driver.action("connection_scroll", y=-1000000)
        scrolled = driver.wait(
            lambda state: state["modal"] == "connections"
            and state["connection_library"]["max_y"] > 0
            and state["connection_library"]["offset_y"] <= -state["connection_library"]["max_y"] + 1,
            "connection list scroll offset",
        )
        assert scrolled["connection_library"]["scrollbar_bounds"]
        checks.append("connection scrollbar follows the list scroll handle")
        checks.append("connection scrollbar thumb moves with the list offset")
        driver.action("keystroke", key="escape")
        driver.wait(lambda state: state["modal"] is None, "connection Escape closes empty modal")
        checks.append("connection Escape closes when no row is selected")
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
    (directory / "modal-results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"checks": checks, "report": str(directory / "modal-results.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
