#!/usr/bin/env python3
"""Verify a random loopback connection's real local-vault credential reuse in an isolated native app.

The fixture must explicitly enable MANTASH_FIXTURE_PASSWORD. This script uses only
its randomly generated connection UUID and password. It never accesses a system keychain.
Run exercise, restart the app with the same QA directory, then run restore. The encrypted test vault remains inside the isolated QA directory.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

from qa_accept_macos import Acceptance


class CredentialAcceptance(Acceptance):
    """Keep user-facing prompt observations separate from credential contents."""

    def __init__(self, directory, fixture):
        super().__init__(directory, fixture)
        self.report = directory / "credential-results.json"
        self.results = json.loads(self.report.read_text()) if self.report.exists() else {"checks": [], "phases": []}

    def settle(self):
        """Wait for current state before asserting focus or prompt absence."""
        self.action("draw")
        self.action("snapshot")

    def write_secret_fixture(self, filename, data):
        """Restrict temporary fixture input permissions and never put it in the command file."""
        path = self.root / filename
        with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "wb") as output:
            output.write(data)
        return path

    def exercise(self):
        """Save once, then reconnect and open another instance through production UI actions."""
        profile = json.loads((self.fixture / "profile.json").read_text())["profile"]
        assert profile["host"] in ["127.0.0.1", "::1", "localhost"]
        # A fresh ID is never shared with another suite or a user's saved connection.
        profile.update(id=str(uuid.uuid4()), name="Credential persistence fixture")
        self.results["profile"] = profile
        self.save()
        password = (self.fixture / "password.txt").read_bytes()
        secret_file = self.write_secret_fixture("fixture-password.txt", password)
        self.wait(lambda s: len(s["tabs"]) == 1, "Fresh isolated workbench")
        self.action("profile", profile=profile)
        self.action("submit_profile", connect=False)
        self.action("open_saved", id=profile["id"])
        self.wait(lambda s: s["modal"] == "trust", "Saved password profile opens directly into host verification")
        self.action("trust")
        state = self.wait(lambda s: s["modal"] == "credentials", "Unsaved password requested only after host verification")
        self.check(state["credential_prompt"]["reason"] == "Missing", "Password prompt appears with the missing-password reason (saving is always on; the toggle was removed)")
        owner = self.pane(state)["owner"]
        attempt = self.pane(state)["attempt"]
        self.action("fixture_credentials", file=str(secret_file))
        self.settle()
        self.action("keystroke", key="enter")
        self.wait(lambda s: self.pane(s)["state"] == "Connected", "One password entry authenticates and saves without a master-password form")
        self.check("凭据已保存" in (self.state()["notice"] or ""), "Encrypted local credential write reports success")
        self.action("profile", profile=profile)
        self.wait(lambda s: s["modal"] == "profile" and s["connection_library"]["form"]["secret_present"] and not s["connection_library"]["form"]["secret_loading"], "Edit form loads the saved password")
        self.check(self.state()["connection_library"]["form"]["existing"], "Saved connection opens the edit form with its password")
        self.action("dismiss")
        self.action("reconnect")
        self.wait(lambda s: self.pane(s)["state"] == "Connected" and self.pane(s)["attempt"] != attempt,
                  "Reconnect reads the saved local credential without entering a password")
        self.check(self.pane(self.state())["owner"] == owner and self.state()["modal"] is None,
                   "Reconnect preserves the pane and does not open a password form")
        self.action("close_tab", index=1)
        self.action("open_saved", id=profile["id"])
        self.wait(lambda s: len(s["tabs"]) == 2 and self.pane(s)["state"] == "Connected", "Reopening the saved connection also reuses credentials")
        self.check(self.state()["modal"] is None, "Saved connection opening has no redundant form")
        self.results["restored_owner"] = self.pane(self.state())["owner"]
        self.results["first_pid"] = self.state()["pid"]
        self.results["phases"].append("exercise")
        self.save()
        # Plaintext appears only in the explicitly created fixture input, not normal app state.
        for filename in ["state.json", "command.json", "data/mantash.sqlite3", "data/mantash.sqlite3-wal", "data/credentials-local.sqlite3", "data/credentials-local.sqlite3-journal", "data/credentials.key"]:
            path = self.root / filename
            if path.exists(): self.check(password not in path.read_bytes(), "No plaintext password in " + filename)
        self.action("quit")

    def restore(self):
        """Verify local-vault persistence in a new process and scope cancellation to its attempt."""
        profile = self.results["profile"]
        self.wait(lambda s: s["pid"] != self.results["first_pid"] and len(s["tabs"]) == 2, "New process restores the previous workspace")
        self.action("tab", index=1)
        self.check(self.pane(self.state())["state"] == "Restored", "Restored SSH still waits for user reconnection")
        self.check(self.state()["vault_state"]=="Automatic", "Restart needs no master-password unlock")
        self.action("reconnect")
        self.wait(lambda s: self.pane(s)["state"] == "Connected", "New process reconnects with the saved local-vault password")
        self.check(self.state()["modal"] is None and self.pane(self.state())["owner"] == self.results["restored_owner"], "Restart preserves pane identity and needs no password form")
        unsaved = {**profile, "id": str(uuid.uuid4()), "name": "Unsaved credential fixture"}
        self.action("profile", profile=unsaved)
        self.action("submit_profile", connect=False)
        self.action("tab", index=0)
        self.action("open_saved", id=unsaved["id"], background=True)
        self.wait(lambda s: len(s["tabs"]) == 3 and s["tabs"][2]["panes"][0]["state"] == "CredentialsRequired",
                  "Unsaved background connection waits for credentials")
        self.check(self.state()["modal"] is None and self.state()["active_tab"] == 0, "Background password requests do not steal focus")
        self.action("tab", index=2)
        self.wait(lambda s: s["modal"] == "credentials", "Switching to the waiting pane reveals its credential request")
        old_attempt = self.pane(self.state())["attempt"]
        self.action("dismiss")
        self.wait(lambda s: self.pane(s)["state"] == "Cancelled", "Cancel closes only the pending authentication attempt")
        self.action("reconnect")
        self.wait(lambda s: s["modal"] == "credentials" and self.pane(s)["attempt"] != old_attempt, "Retry uses a fresh attempt with a fresh prompt")
        self.action("dismiss")
        self.action("close_tab", index=2)
        self.action("tab", index=1)
        self.check(self.pane(self.state())["state"] == "Connected", "Saved connection stays available after cancelling another one")
        self.results["phases"].append("restore")
        self.save()
        self.action("quit")


def run_native(directory, fixture, binary):
    """Own two isolated app processes and retain their encrypted test vault."""
    if sys.platform != "darwin":
        raise SystemExit("This acceptance runner requires macOS.")
    if not binary.is_file() or not (fixture / "password.txt").is_file():
        raise SystemExit("Build the debug app and start the explicitly enabled loopback fixture first.")
    directory.mkdir(parents=True, exist_ok=False)
    exits = []
    try:
        for phase in ["exercise", "restore"]:
            environment = os.environ.copy()
            environment.update(MANTASH_DATA_DIR=str(directory / "data"),
                               MANTASH_QA_CONTROL=str(directory / "command.json"),
                               MANTASH_QA_HIDDEN="1", MANTASH_QA_BACKGROUND="1")
            with (directory / (phase + ".log")).open("w") as log:
                process = subprocess.Popen([str(binary)], env=environment, stdout=log, stderr=subprocess.STDOUT)
                qa = CredentialAcceptance(directory, fixture)
                try:
                    qa.wait(lambda state: state["pid"] == process.pid, "Task-owned native process started")
                    getattr(qa, phase)()
                except Exception:
                    qa.results["failed_phase"] = phase
                    qa.save()
                    raise
                finally:
                    if process.poll() is None:
                        command = directory / "command.json"
                        quitting = command.exists() and json.loads(command.read_text()).get("action") == "quit"
                        if not quitting:
                            qa.action("dismiss")
                            qa.action("quit")
                        process.wait(timeout=15)
                    exits.append(process.returncode)
                    assert process.returncode == 0, "The test app did not exit cleanly"
    finally:
        report = directory / "credential-results.json"
        if report.exists():
            record = json.loads(report.read_text())
            profile = record.get("profile")
            if profile:
                assert profile["name"] == "Credential persistence fixture"
                assert profile["host"] in ["127.0.0.1", "::1", "localhost"]
                record["test_vault"] = "retained in isolated data directory; no keychain access"
                record["native_exit_codes"] = exits
                report.write_text(json.dumps(record, ensure_ascii=False, indent=2))
    record = json.loads((directory / "credential-results.json").read_text())
    print(json.dumps({"checks": len(record["checks"]), "phases": record["phases"], "report": str(directory / "credential-results.json")}))


def main():
    """Require explicit fixture paths; print only counts and the non-secret result location."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=["run", "exercise", "restore"])
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path(__file__).resolve().parents[1] / "target/debug/mantash")
    args = parser.parse_args()
    if args.phase == "run":
        run_native(args.directory.resolve(), args.fixture.resolve(), args.binary.resolve())
        return
    check = CredentialAcceptance(args.directory, args.fixture)
    getattr(check, args.phase)()
    print(json.dumps({"checks": len(check.results["checks"]), "report": str(check.report)}))


if __name__ == "__main__":
    main()
