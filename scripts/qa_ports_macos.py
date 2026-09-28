#!/usr/bin/env python3
"""Exercise the real native ports dialog against isolated, bounded QA socket rows."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from qa_overview_layout_macos import Driver


def ports(count=48):
    """Include duplicate endpoints, UDP, IPv6, unknown ports and long raw process data."""
    rows = [
        {"protocol": "tcp", "state": "LISTEN", "local": "0.0.0.0:22", "peer": "0.0.0.0:*",
         "process": "users:((sshd,pid=40,fd=4))"},
        {"protocol": "tcp6", "state": "LISTEN", "local": "[::]:22", "peer": "[::]:*", "process": ""},
        {"protocol": "udp6", "state": "UNCONN", "local": "[::1]:5353", "peer": "[::]:*", "process": ""},
        {"protocol": "udp", "state": "UNCONN", "local": "*:ssh", "peer": "*:*", "process": ""},
        {"protocol": "tcp", "state": "LISTEN", "local": "0.0.0.0:22", "peer": "0.0.0.0:*",
         "process": "users:((sshd,pid=40,fd=4))"},
    ]
    rows += [
        {"protocol": "tcp" if index % 2 else "udp", "state": "LISTEN" if index % 2 else "UNCONN",
         "local": f"[2001:db8:1234:5678:abcdef:1234:{index:x}]:{2000 + index}",
         "peer": "[::]:*", "process": "users:((very-long-diagnostic-process-name-without-spaces-" + "x" * 70 + ",pid=81,fd=7))"}
        for index in range(count - len(rows))
    ]
    return rows


def draw_ports(driver, label):
    """Wait for a fresh GPUI prepaint, not just a QA command acknowledgement."""
    state = driver.wait(lambda s: s.get("port_view") is not None, label + " opened")
    prior = state["port_view"]["revision"]
    driver.action("draw")
    return driver.wait(lambda s: s.get("port_view") is not None
                       and s["port_view"]["revision"] > prior, label + " repainted")


def assert_toolbar(bounds):
    """Protocol controls and status share a row without collision."""
    toolbar, controls = bounds["toolbar"], bounds["controls"]
    filters, status = bounds["filters"], bounds["status"]
    assert controls["y"] >= toolbar["y"] and controls["y"] + controls["height"] <= toolbar["y"] + toolbar["height"] + 1
    assert filters["x"] >= toolbar["x"] + 12 - 1 and filters["x"] + filters["width"] <= status["x"] + 1
    assert status["width"] > 16 and status["x"] + status["width"] <= toolbar["x"] + toolbar["width"] - 12 + 1


def check_layout(state, width, height, size):
    """Measure the native frame, fixed chrome, column bounds and actual scroll viewport."""
    frame = state["dialog_frame_bounds"]
    view = state["port_view"]
    bounds = view["geometry"]
    assert state["width"] == width and state["height"] == height, state
    assert frame and frame["width"] + 2 <= 640 and frame["height"] + 2 <= 560, frame
    assert frame["y"] >= 23 and frame["y"] + frame["height"] <= height - 23, frame
    assert abs(frame["x"] + frame["width"] / 2 - width / 2) <= 2, frame
    assert abs(frame["y"] + frame["height"] / 2 - height / 2) <= 2, frame
    assert bounds["toolbar"]["y"] < bounds["columns"]["y"] < bounds["viewport"]["y"], bounds
    assert_toolbar(bounds)
    viewport = bounds["viewport"]
    assert viewport["height"] > 100 and viewport["y"] + viewport["height"] <= frame["y"] + frame["height"] + 2, bounds
    for key in ("address", "protocol", "process"):
        assert bounds[key]["width"] > 20, (size, bounds)
        assert bounds[key]["x"] >= viewport["x"] - 1, (size, bounds)
        assert bounds[key]["x"] + bounds[key]["width"] < viewport["x"] + viewport["width"] + 2, bounds
    assert bounds["address"]["x"] + bounds["address"]["width"] <= bounds["protocol"]["x"] + 1, bounds
    assert bounds["protocol"]["x"] + bounds["protocol"]["width"] <= bounds["process"]["x"] + 1, bounds
    assert view["max_y"] > 0 and view["bounds"]["height"] > 100, view
    return {"viewport": [width, height], "ui_size": size, "frame": frame,
            "toolbar": bounds["toolbar"], "columns": bounds["columns"], "list": bounds["viewport"]}


def check_sparse(state, count):
    """Sparse samples must fit every collapsed row below aligned fixed column headings."""
    frame = state["dialog_frame_bounds"]
    view = state["port_view"]
    geometry = view["geometry"]
    viewport = geometry["viewport"]
    first = geometry["first_row"]
    assert_toolbar(geometry)
    assert view["count"] == count and len(view["rows"]) == count, view
    assert frame["height"] < 400 and view["max_y"] <= 1 and view["max_x"] <= 1, (frame, view)
    assert first["y"] >= viewport["y"] - 1
    assert first["y"] + count * first["height"] <= viewport["y"] + viewport["height"] + 1, (frame, first, viewport)
    for name, cell in (("port", "row_port"), ("address", "address"),
                       ("protocol", "protocol"), ("process", "process")):
        header = geometry[f"header_{name}"]
        row = geometry[cell]
        assert abs(header["x"] - row["x"]) <= 1 and abs(header["width"] - row["width"]) <= 1, (name, header, row)


def main():
    """Launch a standalone debug process and retain evidence in a fresh scratch directory."""
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
        layout = []
        try:
            driver.wait(lambda s: bool(s["tabs"]), "native startup")
            driver.action("ports_fixture", ports=ports())
            state = draw_ports(driver, "ports fixture")
            assert state["port_view"]["count"] == 48 and len(state["port_view"]["rows"]) == 48
            assert state["port_view"]["rows"][0]["number"] == 22
            driver.action("ports_loading", initial=True)
            state = draw_ports(driver, "first sample loading")
            assert state["port_view"]["refreshing"] and state["port_view"]["connected"]
            assert state["port_view"]["count"] == 0 and not state["port_view"]["rows"]
            driver.action("ports_fixture", ports=ports())
            state = draw_ports(driver, "first sample complete")
            assert not state["port_view"]["refreshing"] and state["port_view"]["count"] == 48
            driver.action("ports_loading", initial=False)
            state = draw_ports(driver, "refresh pending")
            assert state["port_view"]["refreshing"] and state["port_view"]["count"] == 48
            assert len(state["port_view"]["rows"]) == 48
            driver.action("ports_fixture", ports=ports())
            state = draw_ports(driver, "refresh complete")
            assert not state["port_view"]["refreshing"] and len(state["port_view"]["rows"]) == 48
            checks.append("first sample loading and in-place refresh preserve expected rows")
            for width, height, size, night in [(1280, 800, 14, False), (960, 640, 18, False),
                                               (1280, 800, 18, True), (960, 640, 14, True)]:
                driver.action("resize", width=width, height=height)
                driver.action("font_sizes", ui=size, terminal=size)
                driver.action("theme", night=night)
                state = draw_ports(driver, "layout")
                layout.append({**check_layout(state, width, height, size), "night": night})
            checks.append("native frame and nonoverlapping columns: two sizes, two fonts, both themes")
            for size, night in ((14, False), (18, True)):
                driver.action("font_sizes", ui=size, terminal=size)
                driver.action("theme", night=night)
                for count in (3, 5):
                    driver.action("ports_fixture", ports=ports()[:count])
                    driver.action("ports_scroll", y=0)
                    state = draw_ports(driver, f"{count} compact rows")
                    check_sparse(state, count)
                    checks.append(f"{size}px: {count} rows fit without scrolling and align with sort headers")
                    if count == 3:
                        compact = state["dialog_frame_bounds"]["height"]
                        driver.action("ports_toggle", index=0)
                        expanded = draw_ports(driver, "sparse row expansion")
                        assert expanded["port_view"]["rows"][0]["expanded"]
                        assert expanded["dialog_frame_bounds"]["height"] > compact + 100
                        assert expanded["port_view"]["max_x"] <= 1
                        checks.append(f"{size}px: expanding a sparse row provides readable detail height")
                driver.action("ports_fixture", ports=[])
                empty = draw_ports(driver, "zero ports compact frame")
                assert empty["port_view"]["count"] == 0
                assert empty["dialog_frame_bounds"]["height"] < 260
                checks.append(f"{size}px: empty sample has a compact frame")
            driver.action("language", english=True)
            driver.action("font_sizes", ui=18, terminal=18)
            driver.action("ports_fixture", ports=ports()[:3])
            driver.action("ports_scroll", y=0)
            english = draw_ports(driver, "18px English headings")
            check_sparse(english, 3)
            assert english["port_view"]["geometry"]["header_protocol"]["width"] == 144
            checks.append("18px English toolbar and protocol/state column remain aligned")
            driver.action("language", english=False)
            driver.action("ports_fixture", ports=ports())
            driver.action("ports_scroll", y=0)
            driver.action("ports_query", query="2000")
            state = draw_ports(driver, "long process")
            assert len(state["port_view"]["rows"]) == 1
            compact = state["port_view"]["geometry"]["first_row"]["height"]
            driver.action("ports_toggle", index=0)
            state = draw_ports(driver, "expanded long process")
            assert state["port_view"]["rows"][0]["expanded"]
            assert state["port_view"]["geometry"]["first_row"]["height"] > compact + 24
            assert state["port_view"]["max_x"] == 0
            driver.action("ports_query", query="")
            state = draw_ports(driver, "restored list")
            checks.append("full long process expands within the scroll viewport without horizontal overflow")
            before = state["port_view"]["geometry"]
            driver.action("ports_scroll", y=-100000)
            state = draw_ports(driver, "scroll")
            assert state["port_view"]["offset_y"] < -10
            assert abs(state["port_view"]["offset_y"] + state["port_view"]["max_y"]) < 2
            assert state["port_view"]["geometry"]["toolbar"] == before["toolbar"]
            assert state["port_view"]["geometry"]["columns"] == before["columns"]
            assert state["port_view"]["geometry"]["scrollbar"] == before["scrollbar"], (
                before["scrollbar"], state["port_view"]["geometry"]["scrollbar"]
            )
            checks.append("only rows scroll; fixed toolbar and column headings")
            driver.action("ports_scroll", y=0)
            driver.action("ports_query", query="sshd")
            state = draw_ports(driver, "search")
            assert [r["local"] for r in state["port_view"]["rows"]] == ["0.0.0.0:22"] * 2
            driver.action("ports_toggle", index=0)
            state = draw_ports(driver, "expand")
            assert state["port_view"]["rows"][0]["expanded"]
            driver.action("ports_copy", index=0, number=False)
            assert driver.wait(lambda s: s["port_view"]["copied"] == "0.0.0.0:22", "copied endpoint")
            driver.action("ports_copy", index=0, number=True)
            assert driver.wait(lambda s: s["port_view"]["copied"] == "22", "copied numeric port")
            driver.action("ports_query", query="[::]:*")
            state = draw_ports(driver, "hidden detail")
            assert not any(row["expanded"] for row in state["port_view"]["rows"])
            checks.append("search, duplicate rows, detail, clipboard, hidden detail invalidation")
            driver.action("ports_query", query="")
            driver.action("ports_protocol", protocol="udp")
            state = draw_ports(driver, "udp")
            assert all(row["protocol"].startswith("udp") for row in state["port_view"]["rows"])
            assert any(row["state"] == "UNCONN" and row["local"] == "[::1]:5353" for row in state["port_view"]["rows"])
            driver.action("ports_sort", sort="descending")
            state = draw_ports(driver, "descending")
            driver.action("ports_toggle", index=len(state["port_view"]["rows"]) - 1)
            assert state["port_view"]["rows"][-1]["number"] is None
            driver.action("ports_copy", index=len(state["port_view"]["rows"]) - 1, number=True)
            assert driver.wait(lambda s: s["port_view"]["copied"] is None, "non-numeric disabled")
            driver.action("ports_protocol", protocol="tcp")
            state = draw_ports(driver, "tcp")
            assert all(row["protocol"].startswith("tcp") for row in state["port_view"]["rows"])
            checks.append("TCP/UDP variants, UNCONN, numeric descending and unavailable copy")
            driver.action("ports_protocol", protocol="all")
            driver.action("ports_sort", sort="protocol")
            state = draw_ports(driver, "protocol sort")
            assert state["port_view"]["rows"][-1]["number"] is None
            driver.action("ports_fixture", ports=[], error="ss is not installed")
            state = draw_ports(driver, "ss error")
            assert state["port_view"]["error"] and not state["port_view"]["rows"]
            driver.action("ports_fixture", ports=ports(5), system_error="system sample unavailable")
            state = draw_ports(driver, "system sample error")
            assert state["port_view"]["error"] == "system sample unavailable"
            assert not state["port_view"]["rows"] and state["dialog_frame_bounds"]["height"] < 280
            driver.action("ports_toggle", index=0)
            driver.action("ports_copy", index=0, number=False)
            assert driver.wait(lambda s: s["port_view"]["copied"] is None, "system error blocks hidden copy")
            checks.append("system-wide sample errors hide rows and block stale expand/copy targets")
            driver.action("ports_fixture", ports=[])
            state = draw_ports(driver, "zero ports")
            assert state["port_view"]["error"] is None and state["port_view"]["count"] == 0
            driver.action("ports_fixture", ports=ports(5), refresh_error="connection timed out")
            state = draw_ports(driver, "stale sample")
            assert len(state["port_view"]["rows"]) == 5 and state["port_view"]["refresh_error"]
            driver.action("ports_disconnect")
            state = draw_ports(driver, "disconnect")
            assert not state["port_view"]["connected"] and not state["port_view"]["rows"]
            checks.append("ss failure, real zero results, stale refresh and disconnect separated")
            driver.action("keystroke", key="escape")
            driver.wait(lambda s: s["modal"] is None, "ports closed")
            checks.append("Escape closes existing modal")
        finally:
            if process.poll() is None:
                driver.send("quit")
                process.wait(timeout=20)
        assert process.returncode == 0, process.returncode
    report = {"checks": checks, "layout": layout, "binary": str(args.binary.resolve()),
              "scope": "isolated debug GPUI layout; no Linux transport or formal user mouse acceptance"}
    (directory / "ports-report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({"checks": checks, "report": str(directory / "ports-report.json")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
