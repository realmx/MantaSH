# 交付状态

`1.1.5` 是当前源码基线；本页同时保留首个公开版本 `1.0.0` 的历史交付记录。代码接入和可核实的交付结果记录在本页；人工验收范围单独见[验收记录](acceptance.md)。

| 范围 | 当前状态 | 详情 |
|---|---|---|
| 本地终端、SSH/SFTP、编辑、传输、历史、Linux 监控、工作区恢复 | 接入真实后端，自动化覆盖核心数据与协议流程 | [功能范围](features.md)、[实现映射](implementation.md) |
| macOS 原生输入、分屏、文件和窗口操作 | 有用户人工确认；后续版本需按实际环境回归 | [验收记录](acceptance.md) |
| Linux SSH 主机连接、系统面板、进程和端口 | 已确认的人工范围内可用，不等于所有服务器环境均通过 | [验收记录](acceptance.md) |
| Windows x86/x64/ARM64 | 安装器可由 Actions 构建；ConPTY、DPI、输入法、安装与完整运行未实机验收 | [Windows 检查清单](windows.md) |
| 版本与 Release | `Cargo.toml` 基准 `1.0.0`；`master` 的纯文档更新不建 tag，发布相关更新由 workflow 生成 tag。正式 [v1.0.0 Release](https://github.com/realmx/MantaSH/releases/tag/v1.0.0) 的五包及各自 `.sha256` 已匿名下载，10 个附件均返回 HTTP 200，五包 SHA-256 全部匹配 | [发布文档](release.md) |
| Homebrew tap | `realmx/taps/mantash` 的 1.0.0 cask 由 Release workflow 自动更新；macOS 27.0 arm64 / Homebrew 7.0.6 实测强制下载、重装完成，安装后二进制与公开 DMG 一致，版本、架构和 ad-hoc 签名通过核对 | [macOS 安装](macos.md) |

发布包面向 macOS（arm64/x64 DMG，ad-hoc 签名、未公证）和 Windows（x86/x64/ARM64 Inno Setup 安装器，未代码签名），不提供便携 ZIP。macOS Gatekeeper 可阻止新装包首次启动；Homebrew 安装成功不等于已完成系统“仍要打开”后的启动验收。Windows 的 SmartScreen 与实机使用亦未验证。下载与放行方法见[用户手册](user-guide.md)，真实包以 Release 附件为准。

当前发布源码为初始提交 `ffe9d05`，完整构建和 tap 自动更新见 [Release 运行记录](https://github.com/realmx/MantaSH/actions/runs/36393248372)。本地基础检查通过：Rust 104 项测试通过、1 项原生 UI fixture 按配置忽略，25 项发布脚本测试通过；格式、核心编译检查和文档检查通过。两个公开 DMG 均通过 `hdiutil verify`；本机 `spctl` 拒绝首次执行，未进行系统手动放行后的启动验收。

CI、回环服务器、隔离原生 QA、截图和物理键鼠属于不同证据，具体通过范围应与所用源码和脚本对应。文档与依赖许可见[索引](README.md)；出现新缺陷时记录系统、架构、工具链、步骤、预期、实际结果，再针对目标平台修复。
