# 终端机制参考与实现

> 命令示例以 MantaSH 工程路径为准。当前验收状态见[验收记录](acceptance.md)。

项目参考 iTerm2 的公开终端架构和交互机制，继续使用 Rust + GPUI、portable-pty 与 alacritty_terminal；不复制 iTerm2 业务代码或移植其 AppKit 视图。

## 为什么参考 iTerm2

iTerm2 将终端文本作为专门的网格和 GPU 内容处理：Metal 绘制、字形纹理缓存、帧调度和键盘回显路径分别有专门实现，避免每次按键都重新处理整个工作台。参考的官方源码版本为 `ce706b849e8d07332ce36ceab6eeee511e8be2be`：

| 官方来源 | 核对到的机制 |
|---|---|
| [刷新调度](https://github.com/gnachman/iTerm2/blob/ce706b849e8d07332ce36ceab6eeee511e8be2be/sources/PTYSession/iTermUpdateCadenceController.m) | 区分活跃、空闲、可见状态和吞吐；刚处理过按键且吞吐低时及时更新显示 |
| [Metal 驱动](https://github.com/gnachman/iTerm2/blob/ce706b849e8d07332ce36ceab6eeee511e8be2be/sources/MetalRenderer/iTermMetalDriver.m) | GPU 绘制队列、限制同时在途的帧、测量帧耗时和丢帧 |
| [文字渲染](https://github.com/gnachman/iTerm2/blob/ce706b849e8d07332ce36ceab6eeee511e8be2be/sources/MetalRenderer/Renderers/iTermTextRenderer.mm) | ASCII 纹理与非 ASCII 字形纹理页缓存，字体/单元格变化时处理缓存 |
| [官方性能设置](https://iterm2.com/documentation-preferences-general.html) | GPU 绘制，以及大量数据输入时吞吐和延迟之间的取舍 |

这些机制解释成熟终端的交互表现。当前项目使用 GPUI 提供 GPU 绘制，并由自身的终端调度和状态组织满足产品需求。

## 当前实现

| 机制 | 当前实现 |
|---|---|
| 输出事件触发 | 服务写入真实 PTY/SSH 数据后发送按 Owner 归属的输出唤醒，移除界面的固定 32ms 轮询 |
| 合并刷新通知 | 每个会话只保留一次尚未消费的输出通知；隐藏标签继续解析，但不为每个数据块重绘 |
| 与输入分离的维护任务 | 配置、监控和 QA 维护以 250ms 检查，不决定终端回显；事件处理分批让出主线程 |
| 变化行更新 | 接入 alacritty_terminal damage，复制变化行和光标信息，复用未变化行的字形布局；字体、尺寸、选区或搜索变化会正确失效 |
| 触控板滚动 | 累计不足一行的增量，避免逐事件 round 丢失慢速滚动；区分本地回滚、应用鼠标报告和备用屏方向键 |
| 输入兼容 | 修正 Shift/Ctrl/Alt + F1–F4 和常见 Ctrl 字符别名；鼠标移出窗格释放后结束本地拖选 |

变化行测试使用 200×80 网格：单行修改复制 200 个单元格，而完整快照为 16,000 个。测试把每次增量应用到保留帧，并逐格与完整快照比较，覆盖 ANSI 擦除、插入、滚动区、备用屏、中文/组合字符、选区、搜索和尺寸变化。

`OutputWakeup` 的标记和快照确认都在终端网格锁保护范围内协调：新输出不会因为正在确认上一帧而丢失；后台标签再次显示后确认最新快照，随后的输出可以重新发出唤醒。关闭/重连继续依赖 session UUID + attempt UUID，旧结果不会作用于新实例。本地 PTY 的非阻塞 resize 循环与清屏/回填保护见[开发验证](development.md#本地终端与-pty)。

本地终端的网格 resize 在 Unix 与 Windows ConPTY 路径都经 `terminal_io::resize_local_pty` 提交：先让 OS PTY 接受尺寸，成功后才调用 `resize_local` 更新网格；失败不改网格或清除历史。Windows 后台短暂退避后重试，优先处理队列中的输入、关闭和更新的尺寸。仅在没有真实 scrollback、未编辑提示符位于首行且其余行为空时，清除提示符重排产生的历史；已编辑命令及真实输出保留。滚动条只在存在历史行时显示，真实溢出可滚到首尾。网格回归和 macOS Zsh/Bash 原生 PTY 验证覆盖不足、恰好填满、超出视口、清屏及 Vite 式刷新；Windows Git Bash/ConPTY 的截图场景尚未实机复现，不能仅凭调用路径修复确认其全部成因。

## 验证边界

回归覆盖 core、终端绘制与更新、真实本地 PTY、Shell 历史和 SSH/SFTP 场景；这些内容描述测试范围，不代替平台人工验收。项目不宣称提供 iTerm2 的全部功能、帧率或键盘到屏幕延迟；原生 IME、真实触控板、长时间 TUI、多窗口及 Windows 仍按平台清单验收，iTerm2 的专有协议、tmux 集成、即时回放等产品功能不进入本项目范围。

搜索继续使用现有缓冲区匹配逻辑，超长历史的后台搜索和大段粘贴节流不属于当前产品承诺；GPU 在途帧和纹理底层管理由 GPUI 提供。
