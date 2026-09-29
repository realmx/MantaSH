# 能力与原生实现映射

本文是当前实现的能力、模块与验证路径映射。代码接入、自动化验证和实机验收分开记录，完成代码接入不表示双平台已验收；实际验收状态见[验收记录](acceptance.md)。模块职责与 QA 脚本入口见[开发验证](development.md)。

| 能力 | 原生模块与入口 | 验证路径 |
|---|---|---|
| 紧凑并页、连接库、本地全宽；弹窗宽度上限 | `ui/shell.rs`、`ui/tools.rs`、`ui/dialogs.rs`，顶部连接按钮 | 原生窗口三尺寸及大字号；普通 640px、确认 480px、真实在线编辑器 960px 外框测量 |
| 本地最多 5 窗格，混合方向 | `layout.rs`、`model.rs`、`ui/shell.rs`；工具栏与快捷键 | layout_storage 测试、原生恢复/关闭/上限 |
| 动态本地标签、固定 SSH 标签 | `titles.rs`、`integration.rs`、`services.rs`、`events.rs` | 标题纯逻辑及真实 Shell 输出，Windows 待验 |
| 真实本地终端和输入 | `terminal.rs`、`ui/terminal_view.rs`、`platform.rs` | real PTY、ANSI/Unicode/选区搜索测试；IME、鼠标实机 |
| SSH、指纹、密码认证、取消和重连 | `ssh.rs`、`services.rs`、`ui/dialogs.rs` | 回环真实 SSH 密码认证与指纹门禁；外部 Linux 实机 |
| 连接导入导出 | `connections.rs`、`storage.rs`，连接库预览及导出 | 共用 CSV 示例，错误行与重复识别测试 |
| SFTP 浏览与批量操作 | `files.rs`、`file_selection.rs`、`ui/tools.rs` | 路径范围单元测试、真实 SFTP 行选择和批量目标、文件列表双击 |
| 在线编辑、严格编码、草稿保护 | `encoding.rs`、`files.rs`、`ui/tools.rs`、`ui/dialogs.rs` | BOM/二进制/8 MiB、外部修改后重读与最后保存覆盖、失败草稿保留和原生弹窗 |
| 文件和目录传输、取消、重新传输；普通文件大小与环形上传进度 | `files.rs`、`services.rs`、`model.rs`、`ui/tools.rs`、`ui/dialogs.rs` | 原连接 Owner 与路径归属、异步本地大小读取、真实回环 SFTP 进度总量与字节哈希、原生环形图截图像素、两个并发许可、迟到终态测试 |
| 历史查询、复制、填入、执行及删除 | `integration.rs`、`services.rs`、`ui/history.rs` | nonce、Bash 历史保护、共享 SSH 列表与独立本地列表；Shell/平台实际输入 |
| Linux 概览、进程、端口 | `monitor.rs`、`ui/system.rs` | 真实命令解析、双采样、不可用反馈；用户已确认 Linux 模块人工验收 |
| 进程详情及普通/强制结束 | `processes.rs`、`services.rs`、`events.rs`、`ui/system.rs` | 身份变化/受保护 PID/未知结果的自动化；进程模块人工确认，分项故障注入未提供 |
| 右栏偏好和工具记忆 | `layout::tool_width`、`SavedPane`、`Preferences` | 50% 上限不覆盖偏好、会话切换、收起/展开与重启 |
| SQLite 存储与损坏恢复 | `model::Workspace`、`storage::Database` | 损坏图恢复、不截断、外来文件不改动 |
| 字体、字号、语言与日夜 | `ui/theme.rs`、`ui/i18n.rs`、`platform.rs` | 字号范围 12–18px；Windows DPI/字体待验 |
| 品牌、AI 规则与文档 | assets、根目录 AGENTS、docs 与 scripts | SVG/PNG/ICO/ICNS、链接与示例校验、平台记录 |

顶栏无名称字标、仅保留图标；标签支持范围内横向滚动，选中为中性底色加细底线，悬浮整枚标签统一反馈；Tab/Shift+Tab 在终端内交给 Shell。文件列表无复选框，单击单选、Shift 范围选择、Cmd/Ctrl 增减、双击文件夹打开。以上界面契约以[设计规范](design.md)为准。

## 关键类型

- `PaneLayout`：叶子保存 pane UUID；分支保存独立 UUID、方向、比例、first/second。有效图必须恰好包含全部独立窗格。超过 5 个或损坏图按有效会话拆为单标签，不丢弃记录。
- `SavedTab / SavedPane`：保存参数快照、布局、活动窗格、工具开闭及最近页面，运行时恢复新的连接尝试。
- `Owner`：session UUID 与 attempt UUID；重连换 attempt，所有异步结果按完整 Owner 路由。目录/监控/详情有独立 request UUID。
- `Identity`：PID、`/proc/<pid>/stat` 启动 ticks、主机 boot UUID；采样在 ps 前后读取身份，只为一致实例启用结束动作。
- `Action / Outcome`：只允许 Terminate(SIGTERM) 与 Force(SIGKILL)，结果为 Sent/Gone/Changed/Denied/Unsupported/Unknown。不存在自由命令或任意信号输入。
- `Document`：原 Owner、路径、编码、BOM、内容基线与 revision；当前草稿和保存结果独立。`TransferRecord` 固定原连接/尝试和路径，不根据当前标签重绑定。

## 进程操作边界

原生后端通过原 SSH transport 的独立 exec 通道执行固定 Linux 脚本。名称、用户和完整命令仅作显示，不拼进可执行命令。PID 必须为大于 2 的数字；脚本复核 boot UUID、启动 ticks 和内核线程标记。身份不符或不可读拒绝发送，权限遵循远端实际结果。普通操作失败不会自动升级为 KILL。

Sent 表示信号发送成功，随后由有效采样确认原实例消失。期间不乐观删行；最多主动复核 10 秒，未确认显示待手动核实。网络错误显示结果未知。POSIX 身份复核与 kill 之间仍有很短的时间窗口，本实现不宣称 pidfd 级的原子信号语义。用户已确认进程模块人工验收，但没有提供 PID 复用、目标变化、权限不足与连接中断的分项远程记录，不能据此宣称这些故障注入已逐一执行。

## 平台与清理门槛

macOS 自动化和回环 SSH/SFTP 不能代替 Linux 系统数据、Windows ConPTY/DPI、输入法及实际鼠标验证。Windows 使用同一锁文件和 `scripts/check-windows.ps1`，结果由用户实机反馈；Windows 实机验收未完成，不能把 GitHub Actions 构建通过写成实机通过。Linux 进程集成只对专门创建的测试进程操作，不新增本地 VM/容器。

项目默认分支为 `master`，源码基准版本 1.1.5，唯一版本入口是 `Cargo.toml`。清理不涉及系统 SSH 密钥、用户数据或凭据库，不自动配置远程或推送。
