#!/usr/bin/env python3
"""Measure overview/dialog fixtures and exercise their public native keyboard bindings."""
import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import time


def fixture():
    """Provide bounded synthetic Linux samples only to the opt-in rendering driver."""
    samples = []
    for index in range(61):
        samples.append({
            "boot_id": "overview-layout-fixture", "timestamp": 1700000000 + index * 3,
            "system": "Linux", "kernel": "6.8.0", "hostname": "layout-fixture", "uptime": 3600,
            "cpu": [{"name": "cpu", "total": 10000 + index * 100, "idle": 8000 + index * 80,
                     "percent": 20 + 8 * math.sin(index / 5)}] + [
                         {"name": f"cpu{core}", "total": 2500 + index * 25, "idle": 2000 + index * 20,
                          "percent": 20 + core * 2} for core in range(4)],
            "memory": {"total": 8 * 1024**3, "available": 6 * 1024**3,
                       "swap_total": 2 * 1024**3, "swap_free": 2 * 1024**3},
            "network": [{"name": "eth0", "received": index * 384000, "sent": index * 72000,
                         "receive_rate": 128000 + 32000 * math.sin(index / 7), "send_rate": 24000}],
            "disks": [{"filesystem": "/dev/fixture", "mount": "/", "total": 80 * 1024**3,
                       "used": 25 * 1024**3, "available": 55 * 1024**3}],
            "processes": [], "ports": [], "errors": {},
        })
    return samples


class Driver:
    """Wait for actual state and fresh draw measurements, not only command acknowledgement."""
    def __init__(self, directory, process):
        self.directory = directory
        self.process = process

    def wait(self, predicate, label):
        """Poll one known live QA process with a bounded observation deadline."""
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"Native process exited while waiting for {label}")
            try:
                state = json.loads((self.directory / "state.json").read_text())
                if state["pid"] == self.process.pid and predicate(state):
                    return state
            except (FileNotFoundError, json.JSONDecodeError, KeyError):
                pass
            time.sleep(.05)
        raise RuntimeError("Timed out: " + label)

    def send(self, action, **values):
        """Atomically send one action to this isolated application's control file."""
        sequence = time.time_ns()
        temporary = self.directory / "command.tmp"
        temporary.write_text(json.dumps({"sequence": sequence, "action": action, **values}))
        temporary.replace(self.directory / "command.json")
        return sequence

    def action(self, action, **values):
        """Wait for receipt separately from subsequent asynchronous rendering."""
        sequence = self.send(action, **values)
        return self.wait(lambda state: state["sequence"] >= sequence, action)

    def draw(self):
        """Require new GPUI prepaint bounds after the input-free draw request."""
        state = self.action("snapshot")
        previous = state["overview"]["revision"]
        self.action("draw")
        return self.wait(lambda state: state["overview"]["revision"] >= previous + 4,
                         "four fresh overview card measurements")


def main():
    """Launch only a supplied debug binary, retain all QA data, and quit through its own driver."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    directory = args.directory.resolve()
    directory.mkdir(parents=True, exist_ok=False)
    environment = {**os.environ, "MANTASH_DATA_DIR": str(directory / "data"),
                   "MANTASH_QA_CONTROL": str(directory / "command.json"), "MANTASH_QA_BACKGROUND": "1"}
    cases = []
    scroll_cases = []
    with (directory / "stdout.log").open("w") as stdout, (directory / "stderr.log").open("w") as stderr:
        process = subprocess.Popen([str(args.binary.resolve())], cwd=directory, env=environment,
                                   stdout=stdout, stderr=stderr)
        (directory / "process.json").write_text(json.dumps({"pid": process.pid, "binary": str(args.binary.resolve())}) + "\n")
        driver = Driver(directory, process)
        try:
            driver.wait(lambda state: bool(state["tabs"]), "native startup")
            driver.action("overview_fixture", samples=fixture())
            for width, height, ui, panel in [(1280, 900, 14, None), (960, 800, 18, 420),
                                             (960, 800, 18, 320), (960, 800, 12, 320)]:
                driver.action("resize", width=width, height=height)
                driver.wait(lambda state: state["width"] == width and state["height"] == height, "resize")
                driver.action("panel_width", width=panel)
                driver.action("font_sizes", ui=ui, terminal=ui-2)
                state = driver.draw()
                bounds = state["overview"]["bounds"]
                assert state["overview"]["fixture"] and set(bounds) == {"cpu", "memory", "disk", "network"}
                assert all(0 < box["height"] <= max(300, ui * 20) for box in bounds.values()), bounds
                # Full-bleed modules (2026-09-20 redesign) fill the tool panel;
                # the measuring canvas also covers the 1px panel divider.
                assert all(abs(box["width"] - state["tool_width"]) <= 2 for box in bounds.values()), bounds
                assert all(box["x"] >= 0 and box["x"] + box["width"] <= width + 1 for box in bounds.values()), bounds
                ordered = [bounds[key] for key in ("cpu", "memory", "disk", "network")]
                assert all(abs(box["x"] - ordered[0]["x"]) < 2 for box in ordered), bounds
                for first, second in zip(ordered, ordered[1:]):
                    # Modules abut with the shared 1px separator line (2026-09-20
                    # full-bleed redesign) instead of an 8px gap.
                    gap = second["y"] - first["y"] - first["height"]
                    assert -1.5 < gap < 0.5, bounds
                assert all(bounds[key]["height"] <= max(100, ui * 4 + 40)
                           for key in ("cpu", "disk")), bounds
                if panel is None:
                    assert state["tool_width"] == 280, state["tool_width"]
                if ui == 14:
                    assert max(box["y"] + box["height"] for box in bounds.values()) < height, bounds
                expected_rows = {"cpu": ["cpu", "cpu0", "cpu1", "cpu2", "cpu3"], "disk": ["/"]}
                details = []
                for kind, expected in expected_rows.items():
                    driver.action("focus_resource", kind=kind)
                    driver.action("keystroke", key="space" if kind == "disk" else "enter")
                    dialog = driver.wait(lambda value: value["modal"] == "resource_details", kind + " details")
                    dialog = driver.draw()
                    snapshot = dialog["resource_details"]
                    assert snapshot["kind"] == kind and snapshot["rows"] == expected, snapshot
                    assert snapshot["timestamp"] == 1700000180, snapshot
                    viewport = dialog["modal_scroll"]["bounds"]
                    assert viewport["height"] > 40 and viewport["y"] > 24, viewport
                    assert viewport["y"] + viewport["height"] < height - 24, viewport
                    assert dialog["modal_scroll"]["max_x"] == 0, dialog["modal_scroll"]
                    if kind == "disk":
                        assert viewport["height"] < max(140, ui * 9) and dialog["modal_scroll"]["max_y"] == 0, dialog["modal_scroll"]
                    content = dialog["modal_scroll"]["content_bounds"]
                    left = content["x"] - viewport["x"]
                    right = viewport["x"] + viewport["width"] - content["x"] - content["width"]
                    # The shared modal body uses a uniform 12px inset (unified
                    # with the other dialogs); symmetric on both sides.
                    assert abs(left - right) < 1 and abs(left - 12) < 1, dialog["modal_scroll"]
                    # The internal compact-height measurement is no longer
                    # populated (dialogs hug content on their own); the
                    # observable scroll viewport carries the same guarantee.
                    assert viewport["height"] <= height - 48, dialog["modal_scroll"]
                    if ui == 14:
                        assert viewport["height"] < (600 if kind == "cpu" else 320), dialog["modal_scroll"]
                    snapshot["scroll"] = dialog["modal_scroll"]
                    details.append(snapshot)
                    driver.action("keystroke", key="escape")
                    driver.wait(lambda value: value["modal"] is None and value["resource_focus"] == kind,
                                "details closed and focus returned")
                cases.append({"details": details, "viewport": [width, height], "ui_size": ui, "tool_width": state["tool_width"],
                              "bounds": bounds, "draw_revision": state["overview"]["revision"], "passed": True})
            # Long resource fixtures validate the actual scroll range at the supported minimum viewport.
            driver.action("resize", width=960, height=640)
            driver.action("font_sizes", ui=18, terminal=16)
            driver.action("panel_width", width=480)
            long_sample = fixture()[-1]
            long_sample["cpu"] += [{**long_sample["cpu"][1], "name": f"cpu{core}"} for core in range(4, 64)]
            long_sample["disks"] += [{**long_sample["disks"][0], "mount": f"/srv/mount-{index}"} for index in range(1, 12)]
            driver.action("overview_fixture", samples=[long_sample])
            for kind, count in [("cpu", 65), ("disk", 12)]:
                driver.action("resource_details", kind=kind)
                start = driver.draw()
                assert len(start["resource_details"]["rows"]) == count
                assert start["modal_scroll"]["max_y"] > 0 and start["modal_scroll"]["max_x"] == 0, start["modal_scroll"]
                driver.action("scroll_modal", y=-1000000)
                end = driver.draw()
                assert end["modal_scroll"]["bounds"] == start["modal_scroll"]["bounds"]
                assert abs(end["modal_scroll_y"] + end["modal_scroll"]["max_y"]) < 1, end["modal_scroll"]
                scroll_cases.append({"kind": kind, "rows": count, "viewport": [960, 640], "ui_size": 18,
                                     "scroll": end["modal_scroll"], "offset_y": end["modal_scroll_y"], "passed": True})
                driver.action("dismiss")
        finally:
            if process.poll() is None:
                driver.send("quit")
                process.wait(timeout=20)
        assert process.returncode == 0
    report = {"binary": str(args.binary.resolve()), "kind": "native fixture draw geometry and public keyboard dispatch",
              "real_linux_connection": False, "cases": cases, "scroll_cases": scroll_cases, "exit_code": process.returncode}
    (directory / "overview-layout-results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
