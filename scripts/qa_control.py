#!/usr/bin/env python3
"""Send one action to the opt-in, isolated native debug driver and read its status."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


def process_running(pid):
    """Observe the selected QA process without sending a signal or touching another window."""
    if not isinstance(pid, int) or pid <= 0:
        return False
    if os.name == "posix":
        # Launch Services owns bundled app children. A sandbox may reject kill(0)
        # even for an already-exited PID; query process state before that fallback.
        try:
            process = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "stat="],
                                     text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            if process.returncode == 0:
                state = process.stdout.strip()
                return bool(state) and not state.startswith("Z")
            if process.returncode == 1 and not process.stderr.strip():
                return False
        except OSError:
            pass
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def main():
    """Use a fresh sequence number; wait for acknowledgement without assuming async work is done."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action")
    parser.add_argument("--data", default="{}", help="Additional JSON properties")
    parser.add_argument("--file", help="Read additional properties from a JSON file")
    parser.add_argument("--directory", default=str(Path(tempfile.gettempdir()) / "mantash-ui-qa"))
    arguments = parser.parse_args()
    directory = Path(arguments.directory)
    directory.mkdir(parents=True, exist_ok=True)
    try:
        initial_pid = json.loads((directory / "state.json").read_text()).get("pid")
    except (OSError, ValueError):
        initial_pid = None
    initially_running = process_running(initial_pid)
    sequence = time.time_ns()
    command = {"sequence": sequence, "action": arguments.action, **json.loads(arguments.data)}
    if arguments.file:
        command.update(json.loads(Path(arguments.file).read_text()))
    temporary = directory / "command.tmp"
    temporary.write_text(json.dumps(command, ensure_ascii=False))
    os.replace(temporary, directory / "command.json")
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            state = json.loads((directory / "state.json").read_text())
            if state.get("sequence", 0) >= sequence:
                print(json.dumps({key: state[key] for key in ("sequence", "pid", "modal", "modal_stack", "modal_navigation", "notice", "active_tab", "width", "height", "profiles", "history_count")}, ensure_ascii=False))
                return
        except (OSError, ValueError):
            pass
        # Closing the last native window can end the process before the final snapshot write.
        # Report that observation distinctly; the launcher still owns exit-status validation.
        if arguments.action == "quit" and initially_running and not process_running(initial_pid):
            print(json.dumps({"sequence": sequence, "pid": initial_pid, "status": "process_exited_before_ack"}))
            return
        time.sleep(0.1)
    raise SystemExit("Native QA driver did not acknowledge this action within 15 seconds")


if __name__ == "__main__":
    main()
