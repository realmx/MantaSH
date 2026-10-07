# Windows 调试与实机验收

Windows x86、x64 和 ARM64 的公开包为未签名 Inno Setup 安装器，可从 [GitHub Releases](https://github.com/realmx/MantaSH/releases) 获取。**Windows 实机安装与运行仍未验收**；Actions 构建成功不等于下表通过。

## 环境与构建

在 Windows 上安装 Rust 1.89 或更新版本的 MSVC 工具链、Visual Studio Build Tools 的“使用 C++ 的桌面开发”组件及 Windows SDK。x86/ARM64 目标另需对应 target 和编译工具；有 PowerShell 7 时程序优先使用，否则回退系统 PowerShell，再回退 CMD。无需 Node.js 或浏览器前端。

保留源码、`Cargo.lock`、assets、docs、examples 与 scripts，不复制本地 `target/`、`dist/`、数据库或凭据。在项目根目录的 PowerShell 中：

```powershell
rustc --version
rustup show active-toolchain
.\scripts\check-windows.ps1 -Run
```

脚本检查格式、核心与真实 ConPTY 测试，再检查并编译桌面程序；失败即停止，不执行发布打包或签名。若 PowerShell 策略阻止运行脚本，可逐条执行脚本中的 Cargo 命令，不必全局修改执行策略。

桌面入口在 Windows 的 debug/release 构建中均使用 GUI 子系统，从资源管理器或开始菜单启动时不创建额外控制台窗口；应用内本地 Shell 仍通过 ConPTY 运行。打包脚本检查 EXE 的 PE 子系统字段，拒绝将控制台子系统程序打入安装器。

设置中发现的 Git Bash 安装路径按 Windows 原生目录分隔符显示，例如 `C:\Program Files\Git\bin\bash.exe`；旧版本保存的混合分隔符路径仍可启动并匹配当前选项。本地 Git Bash 使用默认提示符时，移除标题控制序列后的起始空行，保留用户名、颜色、目录、Git 分支和 `$` 前的换行。自定义 Git 提示符文件及不匹配默认前缀的提示符保持原样；不修改用户配置文件，也不影响 SSH 或其他 Shell。相关脚本回归可在 macOS 上运行，Windows ConPTY 与原生窗口间距仍需实机验收。

`build.rs` 将 `assets/mantash.ico` 以资源 ID `1` 嵌入 Windows 桌面 EXE，供 GPUI 窗口、任务栏、文件图标、快捷方式和卸载项使用；安装器使用同一 ICO。Windows runner 在打包前检查实际 EXE 中的图标资源，缺失时停止打包。已有快捷方式的显示仍需按下表实机验收。

## 隔离运行

```powershell
$env:MANTASH_DATA_DIR = Join-Path $env:TEMP "mantash-manual-check"
cargo run --locked
```

该变量只作用于当前 PowerShell。不要覆盖 HOME、USERPROFILE 或生产数据目录。

## Git Bash 滚动边界自动验证

在具有交互桌面的 Windows 环境安装 Git Bash、Python 3 与 Node.js，先构建 debug 程序，再运行隔离 QA（目录必须尚不存在，Shell 路径按实际安装位置调整）：

```powershell
cargo build --locked
python scripts/qa_terminal_boundaries.py --binary target/debug/mantash.exe --shell "C:\Program Files\Git\bin\bash.exe" --directory "$env:TEMP\mantash-scroll-qa"
```

脚本只在隔离 debug QA 中指定 Shell，不修改正常偏好；经真实 ConPTY 输出不足、恰好及超出可视高度的内容，检查滚动条几何、首尾可达、清除历史及 Vite 式空行刷新，覆盖两种窗口尺寸。`terminal-boundary-results.json` 记录实际平台、Shell 和通过场景；失败时退出并保留隔离目录中的状态供排查。Node.js 仅供此测试使用，产品运行不需要。此脚本的 macOS Zsh/Bash 运行结果不代表 Windows 通过，程序化滚动也不代表物理鼠标或各 DPI 的验收。

## 实机检查清单

| 范围 | 操作与预期 |
|---|---|
| 启动 | 从资源管理器和开始菜单启动，仅出现 MantaSH 窗口，不弹出额外命令窗口；应用内本地终端仍可输入和执行命令 |
| 应用图标 | 程序 EXE、开始菜单/桌面快捷方式、运行窗口、任务栏和卸载项均显示 MantaSH 图标 |
| 截图证据 | Windows 标题栏、原生按钮、应用图标和无额外命令窗口的截图必须来自 Windows 实机；macOS 截图只能证明 macOS 布局，不替代 Windows 验收 |
| 本地 Shell / ConPTY | PowerShell 7、系统 PowerShell、CMD 回退；中文输入、粘贴、Ctrl+C、颜色、滚动、Vim/less |
| 字体与 DPI | 默认 14px/12px，100%、125%、150%、200% 缩放及字号放大无截断 |
| SSH 与凭据 | 密码、首次/变化指纹、取消、断线/重连、认证后记忆与重启复用 |
| 文件与编辑 | 单选、Shift/Ctrl 多选、双击导航；上传文件/下载文件或目录、重命名、删除、覆盖冲突和保存失败 |
| 编码与历史 | UTF-8、GBK、GB18030、Big5，文件 UTF-16 LE/BE 与 BOM；PSReadLine 报告、填入/执行与敏感输入保护 |
| Linux 远端 | 连接 Linux 主机并与服务器命令核对 CPU、内存、磁盘、网络、进程和端口 |
| 标签、分屏、窗口 | 本地最多五窗格混合布局、焦点/比例/恢复，SSH 不分屏；不同尺寸下标题栏、菜单、标签滚动与关闭保护 |
| 外观与数据 | 中英文/明暗主题恢复；导出 CSV 的明文 `password` 按敏感文件处理 |

macOS/Unix 的 OpenSSH SFTP 子进程测试不是 Windows 实机证据；Windows SFTP 仍须使用明确的测试服务器。记录系统版本、架构、DPI、Rust/Shell 版本、操作步骤、预期与实际结果；失败附首条错误及上下文，不附凭据或生产历史。当前验收状态见[验收记录](acceptance.md)。
