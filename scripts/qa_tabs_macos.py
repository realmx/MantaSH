#!/usr/bin/env python3
"""Check native tab clipping, reveal and action bounds in an isolated debug window.

Uses the existing view driver and real PTYs. Scroll state is controlled through
GPUI's ScrollHandle; this is not an OS mouse/trackpad event injection test.
"""
import argparse
import json
from pathlib import Path
import shlex
import time

from qa_accept_macos import Acceptance


class TabAcceptance(Acceptance):
    """Reuse acknowledged native commands while keeping this focused report separate."""

    def __init__(self, directory):
        super().__init__(directory, directory)
        self.report = directory / "tab-bar-results.json"
        self.results = {"checks": [], "geometry": [], "scope": __doc__}

    @staticmethod
    def geometry_valid(state):
        """Read actual prepaint bounds: tabs may overflow only their middle viewport."""
        header = state["header"]
        strip = header["bounds"]
        controls = header["controls"]
        # A local tab is active in these runs, so the two split controls sit
        # left of the new-terminal button (SSH tabs hide them).
        expected = ["split-right", "split-down", "new-local", "connections", "settings"]
        if set(controls) != set(expected):
            return False
        ordered = [controls[key] for key in expected]
        return (
            strip["width"] > 0
            and strip["x"] + strip["width"] <= ordered[0]["x"]
            and all(c["width"] > 0 and c["x"] >= 0 and
                    c["x"] + c["width"] <= state["width"] for c in ordered)
            and all(a["x"] + a["width"] <= b["x"] for a, b in zip(ordered, ordered[1:]))
            and abs(state["width"] - ordered[-1]["x"] - ordered[-1]["width"] - 12) < 1
        )

    @staticmethod
    def active_visible(state):
        """Require both ends of an ordinary active tab to fit the scroll viewport."""
        header = state["header"]
        if not state["tabs"] or len(header["tabs"]) != len(state["tabs"]):
            return False
        active = header["tabs"][state["active_tab"]]
        strip = header["bounds"]
        return active is not None and active["x"] >= strip["x"] - 1 and (
            active["x"] + active["width"] <= strip["x"] + strip["width"] + 1
        )

    def record_geometry(self, label):
        """Save only layout evidence, excluding terminal content and user state."""
        self.action("draw")
        self.action("draw")
        self.action("snapshot")
        state = self.wait(self.geometry_valid, label)
        self.results["geometry"].append({
            "scene": label, "width": state["width"], "height": state["height"],
            "ui_size": state["preferences"]["ui_size"],
            "tab_count": len(state["tabs"]), "active_tab": state["active_tab"],
            "header": state["header"],
        })
        self.save()

    def exercise(self):
        """Verify overflow, manual position retention, active reveal and font resizing."""
        self.wait(lambda s: len(s["tabs"]) == 1 and self.pane(s)["state"] == "Connected",
                  "隔离窗口启动真实本地 Shell")
        for index in range(20):
            if index:
                self.action("local")
            folder = self.root / f"workspace-{index + 1:02d}"
            folder.mkdir(exist_ok=True)
            self.settled_terminal()
            self.action("type", text="cd -- " + shlex.quote(str(folder)) + "\r")
            self.wait(lambda s: s["tabs"][s["active_tab"]]["title"] == folder.name,
                      f"标签 {index + 1} 使用真实 Shell 动态目录名")
        owners = [[p["owner"] for p in tab["panes"]] for tab in self.state()["tabs"]]
        self.action("draw")
        self.action("draw")
        self.action("snapshot")
        self.wait(lambda s: s["header"]["max_offset_x"] > 0 and self.active_visible(s),
                  "20 个标签产生栏内溢出，新标签自动显示")
        self.record_geometry("1280×800：按钮固定在窗口内，右边距 12px")

        self.action("scroll_tabs", x=0)
        self.wait(lambda s: abs(s["header"]["offset_x"]) < 1,
                  "可滚动到首端查看早期标签")
        self.action("type", text="printf 'tab-strip-background-check\\n'\r")
        self.wait(lambda s: "tab-strip-background-check\n" in self.pane(s)["terminal"]["text"],
                  "标签栏滚动期间真实终端继续处理命令")
        time.sleep(.5)
        self.check(abs(self.state()["header"]["offset_x"]) < 1,
                   "终端输出和标题事件不把手动滚动位置拉回活动标签")
        self.action("tab", index=0)
        self.wait(self.active_visible, "切换到首个标签后完整可见")
        self.action("tab", index=19)
        self.action("draw")
        self.action("draw")
        self.action("snapshot")
        self.wait(self.active_visible, "切换到末尾标签后自动滚入可见范围")

        for width, height, ui, english, night in [
            (960, 640, 14, False, False),
            (1440, 900, 14, False, False),
            (960, 640, 20, True, True),
            (960, 640, 28, True, True),
            (1280, 800, 15, False, False),
        ]:
            self.action("resize", width=width, height=height)
            self.action("font_sizes", ui=ui, terminal=12)
            self.action("language", english=english)
            self.action("theme", night=night)
            label = f"{width}×{height} / {ui}px / {'英文' if english else '中文'}"
            self.record_geometry(label + "：标签限定范围，右侧按钮有边距")
            self.wait(self.active_visible, label + "：活动标签随布局适配可见")
        self.check(owners == [[p["owner"] for p in tab["panes"]] for tab in self.state()["tabs"]],
                   "滚动、窗口和字体变化不重建任何终端会话")

        self.action("settings")
        self.wait(lambda s: s["modal"] == "settings", "设置入口打开原生设置")
        self.action("dismiss")
        self.action("sidebar")
        self.wait(lambda s: s["modal"] == "other", "连接入口打开原生连接库")
        self.action("dismiss")
        self.action("close_tab", index=19)
        self.action("draw")
        self.action("draw")
        self.action("snapshot")
        self.wait(lambda s: len(s["tabs"]) == 19 and self.active_visible(s),
                  "关闭末尾标签后其余标签和新活动标签保持可用")
        self.action("scroll_tabs", x=100000)
        self.action("draw")
        self.action("draw")
        self.action("snapshot")
        self.wait(lambda s: s["header"]["offset_x"] == 0, "滚动首端不越界")
        self.action("scroll_tabs", x=-100000)
        self.action("draw")
        self.action("draw")
        self.action("snapshot")
        self.wait(lambda s: abs(s["header"]["offset_x"] + s["header"]["max_offset_x"]) < 1,
                  "滚动末端不越界")
        self.action("font_sizes", ui=14, terminal=12)
        self.action("tab", index=18)
        self.wait(self.active_visible, "验收结束恢复默认字号与可见活动标签")
        print(json.dumps({"checks": len(self.results["checks"]), "report": str(self.report)},
                         ensure_ascii=False))


def main():
    """Require an explicitly selected, already running isolated native driver."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    TabAcceptance(args.directory).exercise()


if __name__ == "__main__":
    main()
