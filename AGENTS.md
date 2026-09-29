# MantaSH AI 开发规范

本文描述当前产品和工程约束，供 AI 编码助手、自动化代理和维护者使用。它不是修改日志；历史决策不写入本文件。

## 身份与版本

- 用户可见品牌统一为 **MantaSH**；`mantash` 只用于程序包名、可执行文件、路径和技术标识，`MANTASH_*` 只用于环境变量。
- 源码基准版本为 `1.1.10`，唯一源码版本入口是 `Cargo.toml`，major/minor 由维护者手工递增。Rust 代码使用 `CARGO_PKG_VERSION`；发布构建在 runner 上临时同步为 tag 版本，主分支不产生自动版本提交，已发布版本以 tag 和 Release 页为准。SQLite 格式和连接 CSV 格式独立维护。
- 默认分支为 `master`，提交正文必须说明实际修改。`master` 更新仅涉及根目录 Markdown、`docs/**/*.md` 或 `assets/screenshots/` 中的 README 图片时，不创建新版本；否则 `scripts/release_version.py` 对比上个可达的发布 tag 与当前提交，为本次源码提交创建一个 `vX.Y.Z` tag，不产生自动版本提交。`docs/dependency-licenses.csv`、发布脚本及 workflow 的改动不属于纯文档更新。
- 新 tag 由版本 workflow 显式派发 `release.yml`（`GITHUB_TOKEN` 创建的 tag 不依赖 tag push 自动触发）。外部 `v*` tag push 也可发布；`build/**` push 和带平台选择的手动分支构建只上传以 `<基准版本>-dev.<短SHA>` 命名、保留 14 天的 Actions artifacts。

## 产品范围

- 首版包含本地终端、SSH 密码连接、主机指纹验证、CSV 连接库、SFTP、在线编辑、会话编码、命令历史、Linux 监控、多标签、外观和工作区恢复。
- 仅本地终端支持分屏；每个本地标签最多 5 个窗格，可左右、上下和混合分屏。SSH 不提供分屏。
- 本地会话保持全宽。SSH 会话使用终端在上、文件面板在下、右侧仅显示系统概览的并页布局。在线编辑和命令历史使用弹窗。
- 不恢复密钥认证、私钥路径、私钥口令、JSON 连接格式、串口、云同步、插件市场或多层工作区。

## 界面规则

- 顶栏左侧只显示应用图标；标签在图标和固定操作区之间横向滚动，空白标题栏区域保留系统拖窗。标签只能在当前标签栏内水平排序，栏外释放、Esc 和失焦取消排序。
- 标签整枚绘制选中/悬浮底色，SSH 标签状态圆点占位固定；关闭按钮独立响应。动态标题不得改变会话 ID 或连接资料。
- 文件列表不使用复选框：单击单选、Shift 连续选择、Cmd/Ctrl 增减选择、双击文件夹进入、双击文本打开编辑。过期请求和隐藏项不能成为批量目标。
- 连接库支持搜索、CSV 导入导出、编辑、克隆、连接、排序、多选和批量删除。同一资料可以打开多个独立 SSH 标签；连接库连接动作始终新开会话。
- 默认界面字号 14px、终端字号 12px，二者范围均为 12–18px。间距使用 `theme::SPACE_*` 固定阶梯；标准按钮和单行输入框默认 24px，小图标钮 20px，高度不超过 32px。
- 所有可点击控件使用统一按钮包装层，悬停显示手型、禁用显示禁止光标；图标优先使用组件库或 `assets/icons/` 中的 Lucide 资源。

## 后端与安全

- 正式功能必须连接真实后端；模拟数据只出现在测试或明确隔离的 QA fixture 中。
- 网络、文件、数据库和凭据操作不得阻塞 GPUI 主线程。异步事件必须带 Owner、请求 ID 或等价归属，并过滤取消、重连、切换后的旧结果。
- SSH 必须先完成主机身份确认，再读取和发送本地凭据。表单输入的新密码认证成功后才写入本机 AES-256-GCM 凭据库，错误密码不能覆盖已保存值；用户明确确认导入的 CSV 密码按连接 UUID 写库属于独立的导入路径。不设主密码，不调用系统钥匙串。
- 连接 CSV 固定为 `name,host,port,username,password` 五列，导出密码必须按敏感文件处理。不得记录原始键盘输入、密码、私钥、用户数据库、缓存或临时日志。
- 进程结束操作必须复核主机、Owner、请求、PID、启动标识和主机启动标识。发送信号不等于进程已退出，不自动升级或重试。

## 代码与文档

- 沿用现有 Rust、GPUI、SQLite、事件和服务分层。公共 Rust 接口和复杂逻辑写说明；新增界面文案进入 `src/ui/i18n.rs`。
- 代码修改涉及功能、数据、快捷键、平台行为或发布产物时，同步更新用户、功能、数据、开发、AI 或发布文档。文档描述当前实现，不写修改流水账。
- 不为紧凑布局裁切文字、缩小字体或绕过统一控件包装层。不要执行未授权的删除，不覆盖用户改动。

## 验证与证据

- 基础检查：

```sh
cargo fmt --all -- --check
cargo test --locked --no-default-features --all-targets
cargo check --locked --no-default-features --all-targets
python3 scripts/check_docs.py
```

- 正常原生窗口、无输入 `draw`、程序化键盘、指针驱动和平台人工验收是不同证据，不能互相冒充。测试请求被接受不代表异步操作或布局已经完成。
- Windows x86/x64/ARM64、ConPTY、DPI、输入法和安装体验保持未验证；不能把 GitHub Actions 构建通过写成 Windows 实机通过。
- 禁止创建或运行本地 Linux 虚拟机/容器；新增 Linux 验证只使用用户指定的远程环境。发布包签名状态（macOS ad-hoc 签名、未公证；Windows 未代码签名）和平台实机边界见 [发布文档](docs/release.md)。

## 工作区与发布

- `.pi/`、`.zcode/`、`target/`、`dist/`、本地数据库、凭据和临时日志不提交。
- GitHub Release 只面向 macOS 和 Windows：macOS arm64/x64 各提供 ad-hoc 签名、未经公证的 DMG；Windows x86/x64/ARM64 各提供 Inno Setup 安装器（`packaging/windows/MantaSH.iss`），不做代码签名。每包附 `.sha256`；不发布便携 ZIP。
- 发布流程不要求或读取 Apple Developer ID/公证凭据。runner 只在构建时把 `Cargo.toml`、`Cargo.lock` 和 `docs/dependency-licenses.csv` 同步为 tag 版本，不回写主分支；tag 创建早于发包，发布失败时保留。
- 公开包须如实告知 Gatekeeper/SmartScreen 风险，不得冒充 Developer ID 签名、已公证或 Windows 实机验收，也不得建议用 `xattr` 等方式关闭安全检查。可选 Homebrew tap 依赖 `MANTASH_HOMEBREW_TAP` 仓库变量，以及 `TAP_SSH_PRIVATE_KEY`（tap 专用写入部署密钥）或 `TAP_GITHUB_TOKEN` secret；缺配置时跳过且不阻塞发布。
- 发现真实缺陷时修复实现并补充针对性测试；最终报告必须列出实际运行的命令、未执行的检查和剩余限制。
