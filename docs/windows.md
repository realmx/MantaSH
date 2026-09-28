# Windows 调试与实机验收

Windows x86、x64 和 ARM64 的公开包为未签名 Inno Setup 安装器，可从 [GitHub Releases](https://github.com/realmx/MantaSH/releases) 获取。**Windows 实机安装与运行仍未验收**；Actions 构建成功不等于下表通过。

## 环境与构建

在 Windows 上安装 Rust 1.88 的 MSVC 工具链、Visual Studio Build Tools 的“使用 C++ 的桌面开发”组件及 Windows SDK。x86/ARM64 目标另需对应 target 和编译工具；有 PowerShell 7 时程序优先使用，否则回退系统 PowerShell，再回退 CMD。无需 Node.js 或浏览器前端。

保留源码、`Cargo.lock`、assets、docs、examples 与 scripts，不复制本地 `target/`、`dist/`、数据库或凭据。在项目根目录的 PowerShell 中：

```powershell
rustc --version
rustup show active-toolchain
.\scripts\check-windows.ps1 -Run
```

脚本检查格式、核心与真实 ConPTY 测试，再检查并编译桌面程序；失败即停止，不执行发布打包或签名。若 PowerShell 策略阻止运行脚本，可逐条执行脚本中的 Cargo 命令，不必全局修改执行策略。

桌面入口在 Windows 的 debug/release 构建中均使用 GUI 子系统，从资源管理器或开始菜单启动时不创建额外控制台窗口；应用内本地 Shell 仍通过 ConPTY 运行。打包脚本检查 EXE 的 PE 子系统字段，拒绝将控制台子系统程序打入安装器。

## 隔离运行

```powershell
$env:MANTASH_DATA_DIR = Join-Path $env:TEMP "mantash-manual-check"
cargo run --locked
```

该变量只作用于当前 PowerShell。不要覆盖 HOME、USERPROFILE 或生产数据目录。

## 实机检查清单

| 范围 | 操作与预期 |
|---|---|
| 启动 | 从资源管理器和开始菜单启动，仅出现 MantaSH 窗口，不弹出额外命令窗口；应用内本地终端仍可输入和执行命令 |
| 本地 Shell / ConPTY | PowerShell 7、系统 PowerShell、CMD 回退；中文输入、粘贴、Ctrl+C、颜色、滚动、Vim/less |
| 字体与 DPI | 默认 14px/12px，100%、125%、150%、200% 缩放及字号放大无截断 |
| SSH 与凭据 | 密码、首次/变化指纹、取消、断线/重连、认证后记忆与重启复用 |
| 文件与编辑 | 单选、Shift/Ctrl 多选、双击导航；上传文件/下载文件或目录、重命名、删除、覆盖冲突和保存失败 |
| 编码与历史 | UTF-8、GBK、GB18030、Big5，文件 UTF-16 LE/BE 与 BOM；PSReadLine 报告、填入/执行与敏感输入保护 |
| Linux 远端 | 连接 Linux 主机并与服务器命令核对 CPU、内存、磁盘、网络、进程和端口 |
| 标签、分屏、窗口 | 本地最多五窗格混合布局、焦点/比例/恢复，SSH 不分屏；不同尺寸下标题栏、菜单、标签滚动与关闭保护 |
| 外观与数据 | 中英文/明暗主题恢复；导出 CSV 的明文 `password` 按敏感文件处理 |

macOS/Unix 的 OpenSSH SFTP 子进程测试不是 Windows 实机证据；Windows SFTP 仍须使用明确的测试服务器。记录系统版本、架构、DPI、Rust/Shell 版本、操作步骤、预期与实际结果；失败附首条错误及上下文，不附凭据或生产历史。当前验收状态见[验收记录](acceptance.md)。
