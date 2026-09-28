#!/usr/bin/env python3
"""Bounded native PTY stress test using only an explicitly isolated QA instance.

The emit/paste-reader modes are disposable children launched by this test. The
controller samples whole-process RSS/CPU, not screen FPS or physical input latency.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import shlex
import statistics
import subprocess
import sys
import time

from release_version import source_version

def emit(root, slot, duration):
    """Print bounded real output and persist just the test child's PID and row count."""
    marker = root / f"producer-{slot}.json"
    deadline = time.monotonic() + duration
    count = 0
    marker.write_text(json.dumps({"pid": os.getpid(), "rows": 0, "done": False}))
    while time.monotonic() < deadline:
        sys.stdout.write("".join(f"\x1b[3{slot + 1}mload {slot} {count + i:08d}\x1b[0m 中文 é 😀 output test\r\n" for i in range(10)))
        sys.stdout.flush()
        count += 10
        if count % 1000 == 0:
            marker.write_text(json.dumps({"pid": os.getpid(), "rows": count, "done": False}))
        time.sleep(.05)
    marker.write_text(json.dumps({"pid": os.getpid(), "rows": count, "done": True}))
    print(f"STRESS_COMPLETE_{slot}", flush=True)


def paste_reader():
    """Receive a genuine bracketed paste in raw mode and report its exact digest."""
    import termios
    import tty
    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)
        os.write(sys.stdout.fileno(), b"\x1b[?2004hPASTE_RECEIVER_READY\r\n")
        data = bytearray()
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and len(data) < 2 * 1024 * 1024:
            if select.select([fd], [], [], .1)[0]:
                data.extend(os.read(fd, 65536))
                if data.endswith(b"\x1b[201~"):
                    break
        if not data.startswith(b"\x1b[200~") or not data.endswith(b"\x1b[201~"):
            raise RuntimeError("Missing or truncated bracketed paste")
        payload = data[6:-6]
        os.write(sys.stdout.fileno(), f"PASTE_OK_{hashlib.sha256(payload).hexdigest()}\r\n".encode())
    finally:
        os.write(sys.stdout.fileno(), b"\x1b[?2004l")
        termios.tcsetattr(fd, termios.TCSANOW, saved)


def run(root, duration, fixture=None, force_draw=False):
    """Exercise one fresh local QA workbench and record measurable bounds and cleanup."""
    def state():
        """Read only the explicitly enabled native driver's snapshot."""
        return json.loads((root / "state.json").read_text())

    def pane(s):
        """Resolve focus within the active tab."""
        t = s["tabs"][s["active_tab"]]
        return t["panes"][t["active_pane"]]

    def wait(predicate, limit=15):
        """Wait for real async results with a fixed timeout."""
        deadline = time.monotonic() + limit
        while time.monotonic() < deadline:
            s = state()
            if predicate(s):
                return s
            time.sleep(.02)
        raise AssertionError("Native stress condition timed out")

    def action(name, **values):
        """Send one fresh sequence without relying on whichever tab is later selected."""
        sequence = time.time_ns()
        start = time.monotonic()
        temp = root / "command.tmp"
        temp.write_text(json.dumps({"sequence": sequence, "action": name, **values}))
        os.replace(temp, root / "command.json")
        wait(lambda s: s["sequence"] >= sequence)
        return 1000 * (time.monotonic() - start)

    def resources(pid):
        """Read CPU/RSS for the task-owned process only."""
        row = subprocess.check_output(["ps", "-p", str(pid), "-o", "%cpu=,rss=,time="], text=True).split()
        return {"cpu_percent": float(row[0]), "rss_mib": int(row[1]) / 1024, "cpu_time": row[2]}

    def alive(pid):
        """Check whether this known test child still exists; never signal unrelated PIDs."""
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    s = wait(lambda s: len(s["tabs"]) == 1 and pane(s)["state"] == "Connected")
    if s["profiles"] or len(s["tabs"][0]["panes"]) != 1:
        raise AssertionError("Use a fresh local-only QA window")
    pid = s["pid"]
    checks = []
    time.sleep(2)
    samples = [{"seconds": 0, "phase": "baseline", **resources(pid)}]
    script = shlex.quote(str(Path(__file__).resolve()))
    python = shlex.quote(sys.executable)
    action("type", text=f"{python} {script} paste-reader\r")
    wait(lambda s: "PASTE_RECEIVER_READY" in pane(s)["terminal"]["text"])
    payload = "大段粘贴 abc 中文 😀\n" * 4096
    action("paste", text=payload)
    digest = hashlib.sha256(payload.encode()).hexdigest()
    wait(lambda s: digest in pane(s)["terminal"]["text"])
    checks.append({"name": "large bracketed paste", "bytes": len(payload.encode()), "sha256": digest})
    long_text = "x" * 8192
    action("type", text=f"printf '%s' '{long_text}' | wc -c\r")
    wait(lambda s: "8192" in pane(s)["terminal"]["text"])
    checks.append({"name": "8192-character command reaches real Shell"})
    for vertical in [False, True, False, True]:
        action("split", vertical=vertical)
    wait(lambda s: len(s["tabs"][0]["panes"]) == 5 and all(p["state"] == "Connected" for p in s["tabs"][0]["panes"]))
    for slot in range(5):
        action("focus_pane", index=slot)
        action("type", text=f"{python} {script} emit --directory {shlex.quote(str(root))} --slot {slot} --seconds {duration}\r")
    action("local")
    wait(lambda s: pane(s)["state"] == "Connected")
    file_cycles = 0
    file_latencies = []
    transfer_ids = []
    remote_directory = None
    if fixture is not None:
        profile = json.loads((fixture / "profile.json").read_text())["profile"]
        if profile["host"] not in ["127.0.0.1", "::1", "localhost"]:
            raise AssertionError("Mixed workload only allows the dedicated loopback fixture")
        remote_directory = Path(json.loads((fixture / "fixture.json").read_text())["root"]).resolve() / "stress-files"
        remote_directory.mkdir(exist_ok=True)
        (remote_directory / "notes.txt").write_text("Initial stress document\n")
        upload = root / "concurrent-upload.bin"
        upload.write_bytes(bytes(range(256)) * 16384)
        action("profile", profile=profile)
        action("submit_profile", connect=True)
        wait(lambda s: s["modal"] == "trust")
        action("trust")
        wait(lambda s: pane(s)["state"] == "Connected")
        action("tool", tool="files")
    action("tab", index=0)
    latencies = []
    start = time.monotonic()
    cycle = 0
    while time.monotonic() - start < duration:
        action("tab", index=1)
        begin = time.monotonic()
        action("type", text=f"printf 'RESP_%s\\n' {cycle}\r")
        wait(lambda s: f"RESP_{cycle}" in pane(s)["terminal"]["text"])
        latencies.append((time.monotonic() - begin) * 1000)
        if remote_directory is not None and cycle % 5 == 0:
            file_start = time.monotonic()
            action("tab", index=2)
            action("tool", tool="files")
            action("navigate", path=str(remote_directory))
            wait(lambda s: pane(s)["file_path"] == str(remote_directory) and not pane(s)["file_loading"])
            action("open", path=str(remote_directory / "notes.txt"))
            wait(lambda s: len(pane(s)["documents"]) == 1)
            content = f"Mixed-load draft {cycle} 中文\n"
            action("edit", text=content)
            action("save")
            wait(lambda s: not pane(s)["documents"][0]["dirty"] and not pane(s)["documents"][0]["saving"])
            if (remote_directory / "notes.txt").read_text() != content:
                raise AssertionError("Save under load did not reach the actual remote file")
            action("tool", tool="files")
            action("stage_transfer", upload=True, local_paths=[str(upload)])
            staged = wait(lambda s: bool(s["pending_transfers"]))["pending_transfers"]
            transfer_ids.extend(t["id"] for t in staged)
            action("confirm_transfers", overwrite=True)
            wait(lambda s: s["modal"] is None)
            file_latencies.append((time.monotonic() - file_start) * 1000)
            file_cycles += 1
        action("tab", index=0)
        action("focus_pane", index=cycle % 5)
        if force_draw:
            action("draw")
            action("snapshot")
        if cycle % 5 == 0:
            action("scroll_terminal", lines=10)
            action("scroll_terminal", lines=-1000)
        s = state()
        if any(p["terminal"]["history_lines"] > 10_000 for p in s["tabs"][0]["panes"]):
            raise AssertionError("Scrollback exceeded configured bound")
        if cycle % 5 == 0:
            painted = [p["terminal"]["paint_statistics"] for p in s["tabs"][0]["panes"]]
            sample = {"seconds": round(time.monotonic() - start, 1), "phase": "five_panes", **resources(pid),
                      "paints": sum(p["paints"] for p in painted), "rows_shaped": sum(p["rows_shaped"] for p in painted),
                      "file_cycles": file_cycles, "transfer_count": len(transfer_ids)}
            samples.append(sample)
            (root / "stress-progress.json").write_text(json.dumps({"sample": sample, "cycles": cycle, "last_response_ms": latencies[-1]}))
        cycle += 1
        time.sleep(1)
    wait(lambda s: all("STRESS_COMPLETE_" in p["terminal"]["text"] for p in s["tabs"][0]["panes"]), 15)
    children = [json.loads((root / f"producer-{i}.json").read_text()) for i in range(5)]
    checks.append({"name": "five producers completed", "rows": sum(c["rows"] for c in children), "seconds": duration})
    checks.append({"name": "scrollback stays at or below 10000 lines per pane"})
    if remote_directory is not None:
        wait(lambda s: all(t["state"] == "completed" for t in s["transfers"] if t["id"] in transfer_ids) and
             len([t for t in s["transfers"] if t["id"] in transfer_ids]) == len(transfer_ids), 30)
        if hashlib.sha256(upload.read_bytes()).digest() != hashlib.sha256((remote_directory / upload.name).read_bytes()).digest():
            raise AssertionError("Concurrent upload content mismatch")
        checks.append({"name": "SFTP listing, document save and upload under terminal load", "file_cycles": file_cycles, "transfers": len(transfer_ids)})
        action("close_tab", index=2)
    action("close_tab", index=0)
    wait(lambda s: len(s["tabs"]) == 1)
    time.sleep(1)
    if any(alive(child["pid"]) for child in children):
        raise AssertionError("Completed producer process was not reaped")
    checks.append({"name": "producer processes reaped after completion"})
    samples.append({"seconds": round(time.monotonic() - start, 1), "phase": "after_closing_load_tab", **resources(pid)})
    # Closing a still-running foreground job must release that task as well.
    action("type", text=f"{python} {script} emit --directory {shlex.quote(str(root))} --slot 9 --seconds 60\r")
    deadline = time.monotonic() + 10
    while not (root / "producer-9.json").exists() and time.monotonic() < deadline:
        time.sleep(.05)
    active_child = json.loads((root / "producer-9.json").read_text())["pid"]
    action("close_tab", index=0)
    wait(lambda s: not s["tabs"])
    deadline = time.monotonic() + 5
    while alive(active_child) and time.monotonic() < deadline:
        time.sleep(.05)
    if alive(active_child):
        raise AssertionError("Closing a running foreground job left its test process alive")
    checks.append({"name": "closing active tab releases running foreground test process"})
    for _ in range(20):
        action("local")
        wait(lambda s: pane(s)["state"] == "Connected")
        action("close_tab", index=0)
    time.sleep(1)
    samples.append({"seconds": round(time.monotonic() - start, 1), "phase": "after_20_open_close_cycles", **resources(pid)})
    checks.append({"name": "20 open/close cycles completed without an orphan window"})
    result = {"date": time.strftime("%Y-%m-%d"), "version": source_version(), "pid": pid, "checks": checks, "samples": samples,
              "forced_native_draw": force_draw, "file_cycles": file_cycles, "transfer_count": len(transfer_ids),
              "file_workflow_p95_ms": round(sorted(file_latencies)[max(0,int(.95*len(file_latencies))-1)],2) if file_latencies else None,
              "tab_switch_cycles": cycle, "response_p50_ms": round(statistics.median(latencies), 2),
              "response_p95_ms": round(sorted(latencies)[int(.95 * len(latencies)) - 1], 2),
              "response_max_ms": round(max(latencies), 2), "limits": "Includes QA control overhead, not physical keyboard latency or FPS. Results apply to this local workload and debug build."}
    (root / "stress-results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps({"checks": len(checks), "cycles": cycle, "rows": sum(c["rows"] for c in children), "result": str(root / "stress-results.json")}, ensure_ascii=False))
    subprocess.run([sys.executable, str(Path(__file__).with_name("qa_control.py")), "quit", "--directory", str(root)], check=True)


def main():
    """Choose a bounded child producer or controller, using explicit test paths."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["run", "emit", "paste-reader"])
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--slot", type=int, default=0)
    parser.add_argument("--force-native-draw", action="store_true", help="Verify redraw under output even if the test window is occluded")
    parser.add_argument("--fixture", type=Path, help="Optional dedicated loopback metadata for the mixed SFTP workload")
    args = parser.parse_args()
    if not 1 <= args.seconds <= 900:
        parser.error("seconds must be between 1 and 900")
    if args.mode == "paste-reader":
        paste_reader()
    elif args.directory is None:
        parser.error("an isolated directory is required")
    elif args.mode == "emit":
        emit(args.directory, args.slot, args.seconds)
    else:
        run(args.directory, args.seconds, args.fixture, args.force_native_draw)


if __name__ == "__main__":
    main()
