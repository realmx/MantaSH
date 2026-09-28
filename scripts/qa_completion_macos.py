#!/usr/bin/env python3
"""Verify Tab through GPUI's native keymap and a real local Shell in an isolated window.

Start a fresh opt-in QA window first. Keyboard events use Window.dispatch_keystroke;
this checks native dispatch and PTY behavior, not the macOS physical keyboard or IME UI.
"""
import argparse
import json
from pathlib import Path
import shlex
import sys
import time

from qa_accept_macos import Acceptance


class CompletionAcceptance(Acceptance):
    """Keep keyboard regression evidence separate from the full platform acceptance run."""

    def __init__(self, directory):
        super().__init__(directory, directory)
        self.report = directory / "completion-results.json"
        self.results = {"checks": [], "scope": __doc__}

    @staticmethod
    def current_line(state):
        """Use the real cursor row rather than blank padding at the bottom of the grid."""
        terminal = Acceptance.pane(state)["terminal"]
        return terminal["text"].split("\n")[terminal["cursor"]["row"]]

    def press(self, key):
        """Exercise bindings and focus before the terminal's non-text key encoder."""
        return self.action("keystroke", key=key)

    def complete(self, prefix, expected, key="tab"):
        """Verify completion before Enter, and require focus to stay in the terminal."""
        self.action("type", text=prefix)
        self.wait(lambda s: self.current_line(s).endswith(prefix), "输入未补全文本：" + prefix)
        self.press(key)
        self.wait(lambda s: expected in self.current_line(s) and self.pane(s)["terminal"]["focused"],
                  key + " 完成补全且保留终端焦点：" + expected)

    def exercise(self):
        """Cover files, Unicode, directories, command names, Backtab and modal navigation."""
        self.wait(lambda s: len(s["tabs"]) == 1 and self.pane(s)["state"] == "Connected",
                  "隔离窗口启动真实本地 Shell")
        self.settled_terminal()
        owner = self.pane(self.state())["owner"]
        fixture = self.root / "completion-fixtures"
        fixture.mkdir(exist_ok=True)
        (fixture / "completion-target.txt").write_text("fixture\n")
        (fixture / "中文补全.txt").write_text("fixture\n")
        (fixture / "tab-directory").mkdir(exist_ok=True)
        bin_dir = fixture / "bin"
        bin_dir.mkdir(exist_ok=True)
        probe = bin_dir / "mantash_tab_probe"
        probe.write_text("#!/bin/sh\nprintf 'COMMAND_COMPLETION_OK\\n'\n")
        probe.chmod(0o700)
        self.action("type", text="cd -- " + shlex.quote(str(fixture)) + "\r")
        self.wait(lambda s: self.pane(s)["directory"] == str(fixture), "切入专用补全测试目录")
        self.settled_terminal()

        self.complete("printf '%s\\n' completion-ta", "completion-target.txt")
        self.press("enter")
        self.wait(lambda s: "\ncompletion-target.txt\n" in self.pane(s)["terminal"]["text"],
                  "补全结果由真实 Shell 执行输出")
        self.complete("printf '%s\\n' 中文补", "中文补全.txt")
        self.press("ctrl-u")
        self.complete("printf '%s\\n' completion-ta", "completion-target.txt", key="ctrl-i")
        self.press("ctrl-u")
        self.complete("cd tab-di", "tab-directory/")
        self.press("enter")
        self.wait(lambda s: self.pane(s)["directory"] == str(fixture / "tab-directory"),
                  "目录补全后切换真实工作目录")
        self.action("type", text="cd ..\rPATH=" + shlex.quote(str(bin_dir)) + ":\"$PATH\"\r")
        self.settled_terminal()
        self.complete("mantash_tab_pr", "mantash_tab_probe")
        self.press("enter")
        self.wait(lambda s: "\nCOMMAND_COMPLETION_OK\n" in self.pane(s)["terminal"]["text"],
                  "命令名称补全后执行专用测试程序")

        receiver = fixture / "backtab.py"
        receiver.write_text('''"""Read only the expected test keystroke, then restore the owned test PTY."""
import os, select, termios, time, tty
previous = termios.tcgetattr(0)
received = bytearray()
try:
    tty.setraw(0)
    os.write(1, b"\\r\\nBACKTAB_READY\\r\\n")
    deadline = time.monotonic() + 6
    while len(received) < 3 and time.monotonic() < deadline:
        if select.select([0], [], [], max(0, deadline-time.monotonic()))[0]:
            received.extend(os.read(0, 3-len(received)))
finally:
    termios.tcsetattr(0, termios.TCSADRAIN, previous)
print("\\r\\nBACKTAB_BYTES=" + received.hex(), flush=True)
''')
        self.action("type", text=shlex.quote(sys.executable) + " " + shlex.quote(str(receiver)) + "\r")
        self.wait(lambda s: "\nBACKTAB_READY\n" in self.pane(s)["terminal"]["text"],
                  "前台测试程序接管真实 PTY 输入")
        self.press("shift-tab")
        self.wait(lambda s: "BACKTAB_BYTES=1b5b5a" in self.pane(s)["terminal"]["text"] and
                  self.pane(s)["terminal"]["focused"], "Shift+Tab 向前台程序发送完整 Backtab 序列")
        self.settled_terminal()

        original = self.pane(self.state())["terminal"]["text"]
        self.action("compose", text="候选", cursor=2)
        self.wait(lambda s:self.pane(s)["terminal"]["preedit"] == "候选", "候选组合状态已建立")
        self.action("draw")
        self.press("tab")
        self.action("snapshot")
        time.sleep(.3)
        terminal = self.pane(self.state())["terminal"]
        self.check(terminal["preedit"] == "候选" and terminal["focused"] and terminal["text"] == original,
                   "输入法组合期间 Tab 不写入 Shell，也不移走焦点")
        self.action("cancel_composition")

        self.action("ssh_form")
        form = self.wait(lambda s: s["modal"] == "profile", "打开原生连接表单")
        original_focus = form["focused_control"]
        self.press("tab")
        self.wait(lambda s: s["focused_control"] != original_focus and s["modal"] == "profile",
                  "连接表单中 Tab 继续前进到下一个控件")
        self.press("shift-tab")
        self.wait(lambda s: s["focused_control"] == original_focus and s["modal"] == "profile",
                  "连接表单中 Shift+Tab 返回原控件")
        self.action("dismiss")
        self.check(self.pane(self.state())["owner"] == owner, "所有键盘检查保持原终端会话")
        self.action("split", vertical=False)
        before = self.state()["tabs"][0]["active_pane"]
        self.press("ctrl-tab")
        self.wait(lambda s: s["tabs"][0]["active_pane"] != before, "Ctrl+Tab 仍切换本地窗格")
        print(json.dumps({"checks": len(self.results["checks"]), "report": str(self.report)},
                         ensure_ascii=False))


def main():
    """Require an explicitly selected fresh QA directory; never connect to a server."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    CompletionAcceptance(args.directory).exercise()


if __name__ == "__main__":
    main()
