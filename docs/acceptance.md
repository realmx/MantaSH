# 验收记录

本页集中记录实际运行证据和未验证范围。功能入口见[功能导航](features.md)，复现命令见[开发与验证](development.md)。构建、无输入布局测量、程序化输入、正常窗口截图与平台人工验收分别记载，不互相替代；历史结果仅适用于对应构建与环境。

## 已有人工确认

- macOS 原生输入法、本地分屏、文件和窗口操作在用户确认的环境中正常，不扩展为所有系统版本、安装方式或边界场景。
- Linux 远端的基本连接、系统概览、进程操作和端口功能在用户指定的主机范围内通过；尚无 PID 复用、权限不足、主机重启和中途断线的完整分项故障注入记录。
- 连接库的单选、Shift/Cmd 增减选区和批量删除已有人工确认。

## 自动化证据

CI 执行格式、核心测试/编译、文档链接与章节检查及发布辅助脚本测试。真实 PTY、回环 SSH/SFTP、凭据、编码、SQLite 迁移和文件传输分别有专项测试；默认忽略的 `native_qa_fixture` 是需显式启动的长期测试服务器，不是失败的产品测试。完整命令与适用场景见[必做检查](development.md#必做检查)和[测试索引](development.md#按变更选择验证)。

### macOS 原生专项记录

以下记录来自基线 `8e64423` 后的原生功能接入及控件调整，环境为 macOS 27.0.1（26A434）arm64、Rust 1.99.0，使用 debug 程序与隔离数据目录。需要网络的场景使用真实回环 SSH，桥接系统 OpenSSH `sftp-server`，不使用生产凭据或用户数据库。监控布局使用明确隔离的渲染数据，不代表真实 Linux 采样或信号验收。

各行是对应修改完成后的专项结果，不能相加为同一构建的全套通过数：

| 专项脚本或测试 | 已记录结果 | 证据范围 |
|---|---|---|
| `qa_editor_format_macos.py` | 20 项通过 | 真实 SFTP 文本、反向 Unicode 选区、滚动、独立撤销/重做、相邻输入、IME 组合门禁与草稿 |
| `qa_file_actions_macos.py` | 28 项通过 | 复制字节、冲突重试、旧请求拒绝、重命名后的草稿保存、单行删除目标 |
| `qa_file_rows_macos.py` | 28 项通过 | 单选、Shift/Cmd 选择、过滤、导航、键盘和批量目标 |
| `qa_history_connection_modals_macos.py` | 37 项通过的历史记录；后续宽度调整亦有回归 | 历史与连接库布局、选区/Esc、跨标签固定范围、删除取消恢复目标 |
| `qa_history_owner_macos.py` | 15 项通过 | 真实 PTY 填入不执行、明确执行一次、旧实例/隐藏父帧/切标签/重连/前台程序/断线保护 |
| `qa_overview_layout_macos.py` | 4 组布局、2 组滚动通过 | 真实 GPUI 边界和键盘流程，隔离监控渲染数据 |
| `qa_credentials_macos.py run` | 32 项通过 | 主机确认、真实认证、保存、重连、新实例和进程重启复用；表单不预读密码 |
| `qa_transfers_macos.py` | 49 项通过 | 上传/下载字节哈希、取消、覆盖重试、后台继续、并发上限及终态 |
| `qa_about_menu_macos.py` | 双语菜单、重复打开与父帧恢复通过 | 本进程真实 AppKit 菜单触发共享 About，检查可见活动窗口及 Esc |
| `qa_process_list_macos.py` / `qa_process_details_macos.py` | 布局及归属专项通过 | 少量/大量数据、表头、长命令、身份失效与确认取消；不发送真实信号 |
| `qa_ports_macos.py` | 18 项通过 | 空/少量/大量数据、展开/筛选/排序/复制、错误与断线，等待稳定布局帧 |
| `qa_settings_macos.py` | 当前 17 项通过 | 四个 Switch 的指针、完整 Space/Enter 周期和焦点；两主题、多字号、小窗口；root/22 默认、旧偏好兼容、自定义端口、重启与恢复默认 |
| `qa_modal_widths_macos.py` | 当前 58 项通过 | 中英文、12/14/18px、960×640 与 1280×800、内容宽度、视口边距、正文溢出及父帧恢复 |
| `ui::controls::tests` | 3 项通过 | 五种操作色、主题文字对比度至少 4.5:1、外观组合和禁用优先级 |

功能接入阶段的核心测试为 153 通过、1 ignored；加入旧全局端口偏好兼容用例后的最近记录为 154 通过、1 ignored。对应格式、核心/桌面编译和 debug 构建通过。旧设置、旧弹窗宽度检查的较小用例集已由上表后续专项覆盖，不将旧控件内容当作当前界面。

脚本参数、隔离条件及仍需适配的旧脚本见[原生 QA 索引](development.md#原生-qa-脚本索引)。临时日志、控制文件、密码、数据库及逐文件指纹不提交仓库；报告应注明所用源码、二进制和脚本版本。程序化输入和布局测量未完成物理键鼠或全界面像素验收。

## 历史发布与安装验证

首个公开版本 [v1.0.0](https://github.com/realmx/MantaSH/releases/tag/v1.0.0) 对应源码 `ffe9d05`，五目标构建与 Homebrew cask 更新见 [Release 运行记录](https://github.com/realmx/MantaSH/actions/runs/36393248372)。它是历史发布证据，不代表当前源码已经发布或安装通过。

| 检查 | 实际结果 |
|---|---|
| 公开附件 | 五个安装包与五个 `.sha256` 匿名下载均为 HTTP 200，SHA-256 全部匹配 |
| macOS DMG | arm64/x64 两个映像均通过 `hdiutil verify` |
| Homebrew | macOS 27.0 arm64 / Homebrew 7.0.6 上，`brew fetch --cask --force realmx/taps/mantash`、`brew reinstall --cask realmx/taps/mantash` 成功；重复标准安装提示已是最新版 |
| 安装结果 | `/Applications/MantaSH.app` 为 1.0.0、arm64，ad-hoc 签名有效，二进制与公开 DMG 一致 |
| Gatekeeper | `spctl --assess` 拒绝；未记录系统“仍要打开”放行后的首次启动通过 |

包的签名与平台范围统一见[发布文档](release.md)：macOS 为 ad-hoc 签名、未经公证；Windows 安装器未代码签名。Actions 构建成功与 Homebrew 安装成功均不替代目标平台实际运行。

## 尚未验收与剩余限制

- Windows x86/x64/ARM64 安装/卸载、ConPTY、DPI、输入法、文件操作及完整运行，须按 [Windows 清单](windows.md#实机检查清单)逐项检查。
- macOS 新发布包经 Gatekeeper 手动放行后的首次启动、系统文件选择器、物理键鼠和系统 IME 候选框，须以实际操作记录；程序化组合输入只能证明对应数据流。
- 两主题全部状态的正常窗口截图/像素比较、长时间 TUI 与完整平台回归未完成，不能从局部几何检查推断通过。
- 新 Linux 主机的工具/权限差异、进程信号故障注入和真实旧版 SSH 服务器兼容性仍待专项验证；没有新增本地 Linux VM 或容器。
- 元数据回执、文件操作和文档读取/保存已有归属校验及针对性测试，未穷举全部 UI 调度时序。
- 现有依赖弃用警告、未使用分支/变量及 `block` / `proc-macro-error2` 未来兼容提示仍存在。

新增验收须记录提交或 tag、系统/架构、操作路径、预期与实际结果、证据类型和未执行项。不要在报告中包含生产凭据、完整用户数据库或敏感命令历史。
