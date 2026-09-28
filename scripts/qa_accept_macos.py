#!/usr/bin/env python3
"""Run reproducible native acceptance phases against an isolated macOS QA window.

Start a fresh QA directory and the opt-in loopback fixture first. Restart only this
QA process between the exercise and restore phases. The runner uses
real PTY/SFTP and native view actions; it does not represent OS event injection.
"""
import argparse
import json
import os
from pathlib import Path
import time
from qa_control import process_running


class Acceptance:
    """Keep exact acknowledgements, async predicates and persisted evidence separate."""
    def __init__(self, directory, fixture):
        self.root = directory
        self.fixture = fixture
        self.report = directory / "acceptance-results.json"
        self.results = json.loads(self.report.read_text()) if self.report.exists() else {"checks": [], "phases": []}

    def state(self):
        """Read only the driver explicitly selected by the command-line argument."""
        return json.loads((self.root / "state.json").read_text())

    def action(self, name, **values):
        """Write one command and await its sequence without assuming the work finished."""
        initial_pid = self.state().get("pid")
        sequence = time.time_ns()
        temporary = self.root / "command.tmp"
        temporary.write_text(json.dumps({"sequence": sequence, "action": name, **values}))
        os.replace(temporary, self.root / "command.json")
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            state = self.state()
            if state["sequence"] >= sequence:
                return state
            if name == "quit" and not process_running(initial_pid):
                return {**state, "quit_observation": "process_exited_before_ack"}
            time.sleep(.02)
        raise AssertionError(f"Command not acknowledged: {name}")

    def wait(self, predicate, label):
        """Record a pass only after the expected state is actually observed."""
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            try:
                state = self.state()
            except (FileNotFoundError, json.JSONDecodeError):
                time.sleep(.05)
                continue
            if predicate(state):
                self.check(True, label)
                return state
            time.sleep(.05)
        raise AssertionError(label)

    def check(self, condition, label):
        """Persist assertions as they complete so an interrupted run retains its evidence."""
        if not condition:
            raise AssertionError(label)
        if label not in self.results["checks"]:
            self.results["checks"].append(label)
        self.save()

    def save(self):
        """Write test evidence only to the isolated directory until it is reviewed."""
        self.report.write_text(json.dumps(self.results, ensure_ascii=False, indent=2))

    def settled_terminal(self):
        """Wait for a verified prompt and quiet output before comparing IME-only state.

        Connected means the PTY exists; a Shell can still be printing its first prompt.
        Require a stable frame across the driver's snapshot interval instead of assuming
        process creation makes the visible buffer immediately ready for comparison.
        """
        deadline = time.monotonic() + 15
        signature = None
        stable_since = time.monotonic()
        while time.monotonic() < deadline:
            state = self.state()
            terminal = self.pane(state)["terminal"]
            current = (terminal["text"], terminal["cursor"])
            if current != signature or not terminal["command_editing"] or not terminal["text"].strip():
                signature = current
                stable_since = time.monotonic()
            elif time.monotonic() - stable_since >= .35:
                return state
            time.sleep(.05)
        raise AssertionError("Local Shell did not reach a quiet, verified prompt")

    @staticmethod
    def pane(state):
        """Resolve a pane within the active tab, using that tab's saved focus."""
        tab = state["tabs"][state["active_tab"]]
        return tab["panes"][tab["active_pane"]]

    def exercise(self):
        """Cover native input, layout and file failure recovery before a real restart."""
        pane = self.pane
        s = self.wait(lambda s: len(s["tabs"]) == 1 and pane(s)["state"] == "Connected", "首次运行打开真实本地 Shell")
        self.check(s["profiles"] == 0, "验收使用全新连接资料目录")
        self.action("cancel_composition")
        s = self.settled_terminal()
        original = pane(s)["terminal"]["text"]
        self.action("compose", text="nihao", cursor=5)
        self.wait(lambda s: pane(s)["terminal"]["preedit"] == "nihao", "输入法适配器保留未提交组合文本")
        self.check(pane(self.state())["terminal"]["text"] == original, "未提交组合文本没有进入 PTY")
        self.action("compose", text="你好😀", cursor=4)
        self.wait(lambda s: pane(s)["terminal"]["preedit"] == "你好😀", "组合文本更新支持 UTF-16 代理对")
        self.action("cancel_composition")
        self.wait(lambda s: not pane(s)["terminal"]["preedit"], "取消组合不向 Shell 提交内容")
        self.action("type", text="printf '%s\\n' '")
        self.action("compose", text="中文候选", cursor=4)
        self.action("commit_composition", text="中文提交")
        self.action("type", text="'\r")
        self.wait(lambda s: "\n中文提交\n" in pane(s)["terminal"]["text"], "组合提交通过真实 Shell 回显")
        self.action("search", query="中文提交")
        self.wait(lambda s: pane(s)["terminal"]["matches"] > 0, "终端 Unicode 搜索更新")
        self.action("close_search")
        for vertical in [False, True, False, True, False]:
            self.action("split", vertical=vertical)
        self.wait(lambda s: len(s["tabs"][0]["panes"]) == 5 and all(p["state"] == "Connected" for p in s["tabs"][0]["panes"]), "混合分屏五个真实 Shell，第六次被拒绝")
        retained = {p["owner"] for p in self.state()["tabs"][0]["panes"][:4]}
        self.action("close_pane", index=4)
        self.check({p["owner"] for p in self.state()["tabs"][0]["panes"]} == retained, "关闭窗格不更换其余会话")
        self.action("split", vertical=True)
        self.action("focus_pane", index=2)
        self.check(self.state()["tool_width"] is None, "本地标签始终隐藏右栏")

        profile = json.loads((self.fixture / "profile.json").read_text())["profile"]
        if profile["host"] not in ["127.0.0.1", "::1", "localhost"]:
            raise AssertionError("Only a loopback fixture is allowed")
        self.action("profile", profile=profile)
        self.action("submit_profile", connect=True)
        self.wait(lambda s: s["modal"] == "trust", "新 SSH 主机等待指纹确认")
        self.action("trust")
        self.wait(lambda s: pane(s)["state"] == "Connected", "确认后建立真实回环 SSH")
        self.action("split", vertical=False)
        self.check(len(self.state()["tabs"][1]["panes"]) == 1, "SSH 拒绝分屏")
        owner = pane(self.state())["owner"]
        attempt = pane(self.state())["attempt"]
        self.action("panel_width", width=680)
        self.action("resize", width=1440, height=900)
        self.wait(lambda s: s["tool_width"] == 680, "大窗口恢复主动设置的右栏宽度")
        self.action("resize", width=960, height=640)
        self.wait(lambda s: s["tool_width"] == 480 and s["preferences"]["tool_preferred_width"] == 680, "右栏 50% 上限不覆盖偏好")
        self.action("font_sizes", ui=20, terminal=17)
        self.action("language", english=True)
        self.action("theme", night=True)
        self.check(pane(self.state())["owner"] == owner and pane(self.state())["attempt"] == attempt, "尺寸和字体变化不重建 SSH")
        self.action("font_sizes", ui=14, terminal=12)
        self.action("language", english=False)
        self.action("resize", width=1280, height=800)

        remote_root = Path(json.loads((self.fixture / "fixture.json").read_text())["root"])
        path = remote_root / "acceptance-owned.txt"
        path.write_text("baseline 中文\nsecond line\n")
        self.action("navigate", path=str(remote_root))
        self.wait(lambda s: path.name in pane(s)["files"], "SFTP 列表包含真实测试文件")
        self.action("open", path=str(path))
        self.wait(lambda s: len(pane(s)["documents"]) == 1, "SFTP 文本在右侧编辑器打开")
        draft = "draft 中文\nkeep this content\n"
        self.action("edit", text=draft)
        self.wait(lambda s: pane(s)["documents"][0]["dirty"], "编辑草稿产生未保存状态")
        self.action("hide_tool")
        self.action("tab", index=0)
        self.check(self.state()["tabs"][0]["active_pane"] == 2, "切换标签恢复各自窗格焦点")
        self.action("tab", index=1)
        self.action("reopen_tool")
        self.check(pane(self.state())["tool"] == "editor" and pane(self.state())["documents"][0]["dirty"], "跨标签和收起右栏保留编辑草稿")
        path.write_text("externally modified\n")
        self.action("save")
        self.wait(lambda s: not pane(s)["documents"][0]["dirty"] and
                  not pane(s)["documents"][0]["saving"],
                  "最后保存覆盖外部文件修改")
        self.check(path.read_text() == draft and self.state()["modal"] != "conflict",
                   "外部 vi 修改后编辑器保存内容成为远端最终版本")
        self.action("close_document")
        self.wait(lambda s: len(pane(s)["documents"]) == 0,
                  "保存后可关闭编辑器文档")
        path.write_text("externally reopened\n")
        self.action("open", path=str(path))
        self.wait(lambda s: len(pane(s)["documents"]) == 1 and
                  pane(s)["documents"][0]["path"] == str(path),
                  "关闭后重新打开同一路径创建最新读取")
        self.check(path.read_text() == "externally reopened\n",
                   "重新打开前远端内容保持外部最新版本")
        self.action("edit", text=draft)
        self.action("save")
        self.wait(lambda s: not pane(s)["documents"][0]["dirty"] and not pane(s)["documents"][0]["saving"], "实际保存成功才清除修改标记")
        self.check(path.read_text() == draft, "保存内容在服务器文件系统回读一致")
        self.action("edit", text="unsaved before reconnect\n")
        self.action("reconnect")
        self.wait(lambda s: pane(s)["state"] == "Connected" and pane(s)["attempt"] != attempt, "重连更换尝试标识且保留窗格")
        self.check(pane(self.state())["documents"][0]["dirty"], "重连不丢失旧连接草稿")
        self.action("open", path=str(path))
        self.wait(lambda s: len(pane(s)["documents"]) == 2, "重连后重新打开使用独立文档归属")
        self.action("close_tab", index=1)
        self.wait(lambda s: s["modal"] == "close", "关闭会话列出未保存文档影响")
        self.action("dismiss")
        self.action("system_page", page="overview")
        self.action("refresh_monitor")
        self.wait(lambda s: pane(s)["monitor"] and "system" in pane(s)["monitor"]["errors"], "非 Linux SSH 不展示虚构系统数据")
        self.action("tool", tool="files")
        self.action("close_tab", index=1)
        self.action("discard_close")
        # Recreate a clean SSH tab for restart checks with no unsaved-document prompt.
        self.action("profile", profile=profile)
        self.action("submit_profile", connect=True)
        self.wait(lambda s: pane(s)["state"] == "Connected", "已信任主机再次打开无需重复确认")
        self.action("system_page", page="ports")
        self.action("hide_tool")
        self.action("tab", index=0)
        self.action("focus_pane", index=2)
        snapshot = self.state()
        self.results["restore_expected"] = {"tabs": [{"id":t["id"], "layout":t["layout"], "active_pane":t["active_pane"], "owners":[p["owner"] for p in t["panes"]]} for t in snapshot["tabs"]], "pid": snapshot["pid"]}
        self.results["phases"].append("exercise")
        self.save()
        self.action("quit")

    def restore(self):
        """Compare a newly started process against the persisted tab and pane identities."""
        expected = self.results["restore_expected"]
        self.wait(lambda s: s["pid"] != expected["pid"] and all(p["state"] == "Connected" for p in s["tabs"][0]["panes"]), "真实重启后本地 Shell 重新建立")
        actual = self.state()
        for index, tab in enumerate(expected["tabs"]):
            restored = actual["tabs"][index]
            self.check(restored["id"] == tab["id"] and restored["layout"] == tab["layout"] and restored["active_pane"] == tab["active_pane"] and [p["owner"] for p in restored["panes"]] == tab["owners"], f"重启保留标签 {index+1} 的顺序、布局和稳定窗格 ID")
        self.check(actual["tabs"][1]["panes"][0]["state"] == "Restored", "重启 SSH 等待连接而非自动认证")
        self.check(actual["preferences"]["theme"] == "night" and actual["preferences"]["tool_preferred_width"] == 680, "重启恢复主题与右栏偏好")
        self.action("tab", index=1)
        self.check(self.state()["tool_width"] is None, "重启恢复右栏收起状态")
        self.action("reopen_tool")
        self.check(self.pane(self.state())["tool"] == "system" and self.pane(self.state())["system_page"] == "ports", "重新展开恢复系统端口页")
        # Work restoration is always on (the preference was removed), so the
        # acceptance ends here: restart, check, and record the phase.
        self.results["phases"].append("restore")
        self.results["count"] = len(self.results["checks"])
        self.results["limits"] = ["程序化原生动作不等同于物理键鼠和系统输入法候选框", "回环服务器运行 macOS，不替代真实 Linux 系统/进程验收", "Windows 按用户要求暂缓"]
        self.save()
        self.action("quit")


def main():
    """Select one reviewable phase; starting/stopping the native process stays explicit."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=["exercise", "restore"])
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    args = parser.parse_args()
    if not (args.directory / "data").is_dir() or not (args.fixture / "fixture.json").is_file():
        parser.error("Use an existing isolated QA directory and explicit loopback fixture")
    run = Acceptance(args.directory, args.fixture)
    getattr(run, args.phase.replace("-", "_"))()
    print(json.dumps({"phase": args.phase, "checks": len(run.results["checks"]), "results": str(run.report)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
