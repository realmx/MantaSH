# 开发与验证

本页提供可复现的构建、检查和隔离 QA 入口。模块与复杂状态约定见[架构](architecture.md)，用户行为、界面规格和数据格式分别维护在[功能说明](features.md)、[用户手册](user-guide.md)、[设计规范](design.md)和[数据文档](data.md)；实际平台证据见[验收说明](acceptance.md)。

## 构建环境

使用 [Cargo.toml](../Cargo.toml) 声明的 Rust 1.89 或更新的兼容工具链，保留 [Cargo.lock](../Cargo.lock)，用 `--locked` 构建。产品是 Rust/GPUI 原生应用，运行不依赖 Node.js；Python 3 用于文档检查和 QA 脚本。

| 环境 | 要求与入口 |
|---|---|
| macOS | Xcode Command Line Tools 或 Xcode、Rust。GPUI 启用 `runtime_shaders`，运行时使用 Metal，调试构建无需独立 `metal` 命令；安装与人工回归见 [macOS 指南](macos.md) |
| Windows | Rust MSVC 工具链、Visual Studio Build Tools 的 C++ 桌面组件及 Windows SDK；额外架构需对应 target 和编译工具，见 [Windows 指南](windows.md) |
| 回环 SSH/SFTP 测试 | Unix 测试使用真实 Shell、PTY 与系统 OpenSSH `sftp-server`：macOS 为 `/usr/libexec/sftp-server`，Linux 为 `/usr/lib/openssh/sftp-server`。Windows 的 SFTP 实机检查使用明确的测试服务器 |
| 专项工具 | 终端边界脚本需要 Node.js；真实 Vite 场景另需 npm 和已安装的 Vite CLI；截图像素核对与 LOGO 生成需要 Pillow。只有对应场景需要这些工具 |

在仓库根目录构建并启动：

```sh
rustc --version
cargo --version
cargo build --locked
cargo run --locked
```

源码更新后重新编译并正常退出旧实例，已打开的窗口不会自动替换。首次构建需要下载锁定依赖，缓存齐全后可加 `--offline`。默认 `desktop` feature 包含桌面程序；`--no-default-features` 可运行核心和真实协议测试，不构建 GPUI 窗口。

平台辅助脚本 [check-macos.sh](../scripts/check-macos.sh) 与 [check-windows.ps1](../scripts/check-windows.ps1) 顺序执行格式、无桌面测试、桌面检查和 debug 构建；分别加 `--run` / `-Run` 可在检查后启动。它们不打包发布，也不包含下面全部基础检查，文档检查仍须单独执行。

## 必做检查

```sh
cargo fmt --all -- --check
cargo test --locked --no-default-features --all-targets
cargo check --locked --no-default-features --all-targets
python3 scripts/check_docs.py
```

改动桌面代码、资源或 GPUI 依赖时，再在目标桌面平台执行：

```sh
cargo check --locked --all-targets
cargo build --locked
```

修复归属、取消、编码、导入或保存缺陷时补能区分正确/错误行为的针对性测试；文案、尺寸等改动以相关静态检查和原生观察为主，不增加复述实现的测试。检查通过后仅因新修改、失败或未解决疑点扩展验证。

文档检查器验证必需文件、仓库内链接和 Markdown 章节锚点，并检查许可证清单及界面文案键。修改该脚本时运行 `python3 -m unittest discover -s tests -p 'test_check_docs.py'`；CI 同时运行此测试与文档检查。

更新日志生成器和提交前钩子的改动运行 `python3 -m unittest discover -s tests -p 'test_changelog.py'`。此测试在临时 Git 仓库中验证完整历史、合并、手写摘要保护、重复执行，以及钩子的随提交暂存、首次提交、浅克隆、指定路径提交、未暂存修改和跳过开关；不连接远端。钩子启用方式与完整规则见[更新日志维护规则](release.md#更新日志自动生成)。

## 按变更选择验证

下表列出 `tests/` 中现有测试目标及其适用范围。运行单项时用 `cargo test --locked --no-default-features --test <目标名>`；同一命令可重复 `--test` 选择多个目标。测试名称和平台条件以源码为准。

| 变更范围 | 测试目标 | 重点 |
|---|---|---|
| 通用模型、编码、CSV | `core`、`import_monitor_boundaries` | 编码/BOM、文本允许列表与控制字符策略、回退解码、二进制拒绝、跨包多字节、CSV 合并与边界输入、文件列表 mtime 映射与时区格式化、大小/修改时间排序与选区 |
| 元数据与历史 | `metadata_commit`、`shared_history`、`history_migration`、`shell_history` | FIFO、request/revision、密码分阶段失败、格式 2 一致备份/事务失败、各列表稳定 UUID 去重、nonce 与重启 |
| 凭据与 SSH/SFTP | `automatic_vault`、`ssh_sftp` | 主机确认先于凭据读取、认证后保存、错误密码保护、旧回复取消、真实 SFTP 文件及传输；`tests/support/credential_reconnect.rs` 由 `ssh_sftp` 引入 |
| 终端 I/O、网格与输入 | `local_pty`、`terminal_updates`、`terminal_paint`、`local_terminal_clear`、`command_cursor`、`git_bash` | 真实 PTY、连续 resize/关闭、唤醒合并、damage 与完整帧逐格一致、局部复制、Unicode、清屏与滚动边界、提示符定位 |
| 文件选择与编辑 | `file_selection`、`editor_format`、`ssh_sftp` | 隐藏过滤/过期选区、UTF-8 端点、最新读取、最后保存覆盖、权限/取消失败保护及目录传输 |
| 工作区与偏好 | `layout_storage`、`preferences_normalize`、`window_layout` | 十窗格上限、损坏图恢复、格式标记、宽度偏好、默认值及几何、文件列表修改时间与排序表头列宽 |
| Linux 监控与进程 | `monitor_history`、`processes`、`process_command_filter`、`import_monitor_boundaries` | 缺失分段、差值、身份一致、受保护 PID、固定信号、未知结果及 macOS 拒绝 Linux 操作 |
| 自动更新 | `update` | 版本/平台资源选择、校验、安装助手和失败边界，另见[自动更新](updates.md) |

终端机制曾对照 iTerm2 固定提交 `ce706b849e8d07332ce36ceab6eeee511e8be2be` 的[刷新调度](https://github.com/gnachman/iTerm2/blob/ce706b849e8d07332ce36ceab6eeee511e8be2be/sources/PTYSession/iTermUpdateCadenceController.m)、[Metal 驱动](https://github.com/gnachman/iTerm2/blob/ce706b849e8d07332ce36ceab6eeee511e8be2be/sources/MetalRenderer/iTermMetalDriver.m)和[字形缓存](https://github.com/gnachman/iTerm2/blob/ce706b849e8d07332ce36ceab6eeee511e8be2be/sources/MetalRenderer/Renderers/iTermTextRenderer.mm)作机制参考；MantaSH 继续使用自身 Rust/GPUI 实现，不据此承诺相同功能、帧率或输入延迟。

桌面单元测试需启用默认 feature，例如：

```sh
cargo test --locked --lib ui::controls::tests
cargo test --locked --lib ui::updates::tests
```

前者覆盖五种按钮 tone、主题对比度、外观组合和禁用状态，后者覆盖手动/自动检查、重复请求、取消与迟到回复。`cargo test --locked --lib ui::editor::tests` 覆盖重连后文档的复用、重新读取与改绑判定。Switch 的焦点与完整按下/抬起周期、编辑事务的原生撤销/重做则使用下面的专项 QA。

`ssh_sftp` 在回环地址启动随机口令/密钥的测试服务器，SFTP 桥接系统 OpenSSH，读写真实临时文件；这是测试工具，正式应用只作 SSH 客户端。`processes` 的 Linux 条件测试仅操作自己创建的 `sleep 30` 子进程。新增 Linux 验证只使用用户指定的远程环境，禁止创建或运行本地 Linux VM/容器。Windows x86/x64/ARM64、ConPTY、DPI、输入法和安装体验仍未实机验收。

## 原生视图驱动（debug QA）

驱动只编译进 debug 构建，并要求显式设置绝对路径 `MANTASH_QA_CONTROL` 和位于其父目录下的 `MANTASH_DATA_DIR`。正常启动不读取控制文件。重启会忽略旧请求序号，避免重放 Type/Quit。驱动使用真实视图动作和后端，不绕过主机验证；连接资料和凭据入口只接受专用回环目标。合成系统采样由明确的隔离 fixture 动作提供，不能接入正式会话。

### 手动启动与控制

以下 macOS/Unix 示例在全新临时目录运行一个 debug 程序。第一终端打印的目录供控制脚本使用；保留构建产物路径，不覆盖 HOME 等系统环境变量。

```sh
cargo build --locked
mantash_binary="$PWD/target/debug/mantash"
mantash_qa="$(mktemp -d "${TMPDIR:-/tmp}/mantash-ui-qa.XXXXXX")"
printf '%s\n' "$mantash_qa"
(
  cd "$mantash_qa"
  MANTASH_DATA_DIR="$mantash_qa/data" \
  MANTASH_QA_CONTROL="$mantash_qa/command.json" \
  "$mantash_binary"
)
```

第二终端回到仓库根目录，把变量设为刚才打印的实际目录，再发送动作：

```sh
mantash_qa="/实际的隔离目录"
python3 scripts/qa_control.py settings --directory "$mantash_qa"
python3 scripts/qa_control.py font_sizes --directory "$mantash_qa" --data '{"ui":18,"terminal":17}'
python3 scripts/qa_control.py keystroke --directory "$mantash_qa" --data '{"key":"tab"}'
python3 scripts/qa_control.py draw --directory "$mantash_qa"
python3 scripts/qa_control.py dismiss --directory "$mantash_qa"
```

[qa_control.py](../scripts/qa_control.py) 还提供 `resize`、`theme`、`local`、`split`、`focus_pane`、`close_pane`、`profile`、`submit_profile`、`trust`、`reconnect`、`hide_tool`、`reopen_tool` 等动作，具体参数见 `src/ui/qa.rs`。每个应用使用自己的目录，脚本始终显式传 `--directory`。`MANTASH_QA_BACKGROUND=1` 不主动激活窗口；`MANTASH_QA_HIDDEN=1` 隐藏窗口但仍运行原生分发和真实 PTY，只用于适合隐藏运行的检查。

命令回执只代表已接收。连接、读取、保存、退出及几何调整要继续等待 `state.json` 中的目标状态；不能用固定延迟或请求接受代替完成。可观察 `profile_order`、`file_request`/`file_loading`、`file_tree_loading`/`file_tree_rows` 及路径输入框的值/焦点/边界，文件路径回车用 `file_path_input` 配合 `keystroke` 检查。状态含隔离终端可见输出，只允许测试内容，控制文件、状态、临时凭据和日志不提交。

### 回环 SSH/SFTP fixture

另开终端，在仓库根目录启动长期 fixture：

```sh
mantash_fixture="$(mktemp -d "${TMPDIR:-/tmp}/mantash-fixture.XXXXXX")"
printf '%s\n' "$mantash_fixture"
MANTASH_FIXTURE_DIR="$mantash_fixture" MANTASH_FIXTURE_PASSWORD=1 \
  cargo test --locked --no-default-features --test ssh_sftp native_qa_fixture -- --ignored --nocapture
```

该 ignored 项是显式启动的服务器工具，不是被跳过的失败产品测试。最长运行 45 分钟，在其目录创建 `stop-fixture` 可提前停止。`fixture.json`/`profile.json` 给出临时根路径与回环资料；`MANTASH_FIXTURE_PASSWORD=1` 仅把随机密码写入 fixture 的 0600 文件。脚本先通过生产主机确认，再经回环凭据入口读取密码文件，不把密码值写入控制 JSON、状态或报告。此服务器并非文件系统沙箱，测试必须只操作指定 fixture 根下的文件，不连接生产主机。

在脚本终端把 `mantash_fixture` 设为这里打印的实际目录。依赖已运行窗口的脚本用应用目录与 fixture 目录两组参数，例如：

```sh
mantash_fixture="/实际的fixture目录"
python3 scripts/qa_file_actions_macos.py --directory "$mantash_qa" --fixture "$mantash_fixture"
```

传输脚本还要求在**启动 QA 应用的终端**设置同一个 `mantash_fixture` 路径，并将 `MANTASH_QA_REMOTE_ROOT` 设为 `fixture.json` 中 root 的真实路径，再启动应用：

```sh
export MANTASH_QA_REMOTE_ROOT="$(python3 -c 'import json, pathlib, sys; print(pathlib.Path(json.loads(pathlib.Path(sys.argv[1]).read_text())["root"]).resolve())' "$mantash_fixture/fixture.json")"
```

macOS 的 `/var` 与 `/private/var` 必须统一为解析后的路径。`stage_transfer` 只替代系统路径选择器，本地目标须位于 QA 目录、远端目标须在回环 fixture 根下；确认、取消、重试仍走正式入口。它不构成系统文件选择对话框的物理鼠标验收。

## 原生 QA 脚本索引

脚本路径均可点击查看参数。自行启动应用的脚本通常要求 `--binary target/debug/mantash --directory <尚不存在的目录>`；连接现有窗口的脚本要求 `--directory <已运行的隔离目录>`。使用 fixture 的行另加 `--fixture <fixture 目录>`。各场景用独立目录，恢复阶段才复用同一目录；不要把同一窗口同时交给多个驱动。

| 脚本 | 启动方式与依赖 | 覆盖与限制 |
|---|---|---|
| [qa_smoke.py](../scripts/qa_smoke.py) | 现有窗口 + fixture；部分断言待适配 | 先准备第一个标签的 10 个本地窗格、第二个已信任并认证的回环 SSH 标签；分屏、面板、SFTP 与草稿冒烟，保留旧工作页断言 |
| [qa_accept_macos.py](../scripts/qa_accept_macos.py) | 现有窗口 + fixture；`exercise` / `restore`，部分断言待适配 | 组合输入、十窗格、文件重读/覆盖、重连后旧草稿改绑保存与干净文档重新读取和恢复场景；两阶段间实际退出并重启，编辑器工作页断言需适配当前弹窗 |
| [qa_completion_macos.py](../scripts/qa_completion_macos.py) | 现有窗口，真实本地 Shell | 文件/中文路径/目录/命令补全、前台 Backtab、组合输入保护及表单导航 |
| [qa_tabs_macos.py](../scripts/qa_tabs_macos.py) | 现有窗口 | 动态名称、滚动边界、后台输出、活动标签及窗口尺寸；几何来自原生预绘制 |
| [qa_tab_reorder_macos.py](../scripts/qa_tab_reorder_macos.py) | 自行启动 | 排序、栏外/Esc 取消、关闭、滚动、字号和保存；先等待 AppKit 查询原生区域，再比较 GPUI 边界及投递指针事件 |
| [qa_terminal_boundaries.py](../scripts/qa_terminal_boundaries.py) | 自行启动 + Node.js；可传 `--shell` | 两种尺寸下不足/恰好/超出视口、首尾可达、清屏和 Vite 式刷新；Windows 必须指定 Git Bash，报告实际平台/Shell |
| [qa_terminal_scroll_macos.py](../scripts/qa_terminal_scroll_macos.py) | 自行启动；另传 `--vite-cli <vite/bin/vite.js>`，需 Node.js/npm | 真实 Vite、隔离 zsh 右提示符、缩放、标签切换、溢出回滚和运行中清屏；记录网格、历史、滑块及外层滚动几何 |
| [qa_file_rows_macos.py](../scripts/qa_file_rows_macos.py) | 现有窗口 + fixture | 单选、范围/增量选择、隐藏过滤、导航、键盘和批量确认；行事件含 session/attempt/request |
| [qa_file_actions_macos.py](../scripts/qa_file_actions_macos.py) | 现有窗口 + fixture | 固定本行预览/复制/移动/删除、跨目录粘贴与移动、同目录/移入自身/冲突拒绝、取消待移动、旧 request/attempt 拒绝及重命名后草稿保存 |
| [qa_editor_format_macos.py](../scripts/qa_editor_format_macos.py) | 现有窗口 + fixture | 真实 SFTP、Unicode 反向选区、滚动、独立撤销/重做、相邻输入、草稿收起和 IME 门禁；结果只导出内容哈希 |
| [qa_transfers_macos.py](../scripts/qa_transfers_macos.py) | 现有窗口 + fixture；应用启动时设置 `MANTASH_QA_REMOTE_ROOT` | 多文件/目录、字节哈希、目标绑定、覆盖失败、重试、两个并发许可和取消 |
| [qa_modal_widths_macos.py](../scripts/qa_modal_widths_macos.py) | 自行启动 | 中英文、12/14/18px、960×640/1280×800、各类弹窗与父帧恢复；等新 `dialog_revision` 后检查实际外框、中心误差 ≤1px、边距和正文溢出 |
| [qa_history_connection_modals_macos.py](../scripts/qa_history_connection_modals_macos.py) | 自行启动，隔离列表 fixture | 历史/连接库选区、Esc、滚动与底栏，跨标签固定列表范围、删除取消保留目标；不启动传输 |
| [qa_history_owner_macos.py](../scripts/qa_history_owner_macos.py) | 自行启动，真实本地 PTY | 生产历史提交入口的填入/执行、切标签/窗格、隐藏父帧、重开/重连、前台程序与断线；QA 本地历史入口不改变产品入口 |
| [qa_connection_duplicate_macos.py](../scripts/qa_connection_duplicate_macos.py) | 自行启动，回环无监听端口 | 真实 Enter 两次连接同一资料生成两个独立标签；连接会自行失败，不验证 SSH 成功 |
| [qa_connection_row_width_macos.py](../scripts/qa_connection_row_width_macos.py) | 自行启动 | 行填满列表、600px 高度上限及过滤后几何 |
| [qa_process_list_macos.py](../scripts/qa_process_list_macos.py) | 自行启动，隔离采样 | 0/3/90 行、表头对齐、筛选与稳定高度 |
| [qa_process_details_macos.py](../scripts/qa_process_details_macos.py) | 自行启动，隔离采样 | 过期回复、两种确认取消、身份失效、长命令和原始状态；不发送真实信号 |
| [qa_ports_macos.py](../scripts/qa_ports_macos.py) | 自行启动，隔离采样 | 0/3/5/48 行、表头/行对齐、展开/筛选/排序/复制、错误与断线 |
| [qa_overview_layout_macos.py](../scripts/qa_overview_layout_macos.py) | 自行启动，隔离采样 | 概览模块、全宽分隔线、资源详情、滚动边界，以及公开 Space/Enter 焦点入口；不连接 Linux |
| [qa_credentials_macos.py](../scripts/qa_credentials_macos.py) | `run --directory <新目录> --fixture <目录>`；可传 `--binary` | 自动启动和重启，验证隔离密码库保存、重连、新开实例和重启复用；`exercise` / `restore` 可接现有窗口 |
| [qa_about_menu_macos.py](../scripts/qa_about_menu_macos.py) | 自行启动并激活窗口 | AppKit 应用菜单打开共享 About、双语、重复打开、Esc 与设置父帧恢复 |
| [qa_settings_macos.py](../scripts/qa_settings_macos.py) | 自行启动 | 三组设置、四个 Switch 的指针/完整 Space/Enter 周期及 Tab 焦点、两主题/字号、端口 22 默认与旧字段兼容、持久化和恢复默认保留业务数据/自定义端口 |
| [qa_updates_macos.py](../scripts/qa_updates_macos.py) | 自行启动；需低于最新稳定版的 debug 程序及 GitHub 网络 | 官方更新提示、延后显示、手动检查及取消；不确认下载或安装 |
| [qa_stress.py](../scripts/qa_stress.py) | 现有窗口；`run --seconds 600`，可加 `--fixture` | 五窗格真实输出；有 fixture 时每五轮混合 SFTP 刷新、编辑保存和上传；窗口可见，不与其他基准并行 |
| [qa_bundle_macos.py](../scripts/qa_bundle_macos.py) | `--bundle <debug MantaSH.app> --directory <新目录>`，可加 `--fixture` | 搬离源码后的 Launch Services 启动、资源和凭据；校验已有签名，不执行打包或签名，不代表物理双击/Gatekeeper 验收 |
| [qa_window_controls_macos.py](../scripts/qa_window_controls_macos.py) | `--binary`、`--directory`；当前入口待适配 | 保留窗口几何、PTY 身份及重启恢复断言；脚本仍等待 `window_controls` 应用弹窗，而 macOS 当前入口为系统菜单，不能直接作为当前菜单回归通过依据 |
| [check_overview_pixels.py](../scripts/check_overview_pixels.py) | `--image <PNG> --state <state.json>`，需 Pillow | 对同一构建的已呈现窗口截图按 ICC 转换 sRGB，检查轨道填充和网络曲线 |

标为“待适配”的脚本保留作场景参考，使用前须更新其入口断言；当前文件/编辑验证优先选择上述专项脚本，窗口菜单按平台指南检查。脚本覆盖范围与实际执行结果分别记录。

## 测试证据类型

| 证据 | 能说明什么 | 不能代替什么 |
|---|---|---|
| Rust 单元/集成测试 | 模型约束、失败边界；明确使用真实 PTY/回环协议的测试可证明对应服务路径 | 原生布局、目标平台安装和人机输入体验 |
| 正常可见窗口及截图 | 当前构建在实际桌面的呈现和指定像素结果 | 未操作的交互、隐藏窗口或另一平台 |
| 无输入 `draw` | GPUI 实际布局、预绘制边界和滚动视口 | 键盘/指针动作、系统输入法、正常窗口连续呈现 |
| 程序化键盘 | `Window::dispatch_keystroke` 经焦点树、键绑定和输入事件的处理 | 物理键盘、系统输入法候选框；`type` 写制表符也不能代替键盘回归 |
| 程序化指针 | 隔离 macOS 窗口号上的 AppKit NSEvent 路由与视图响应 | 物理鼠标、Window Server 实际拖窗及系统文件选择器 |
| 平台人工验收 | 记录的系统/架构/DPI、Shell、输入法、安装和实际操作 | 未覆盖的平台和分项故障注入 |

布局更改后至少完成两次 `draw`，等待实际边界稳定，再设置滚动偏移和断言；历史高度调整、父弹窗恢复和虚拟列表尤其不能只检查目标尺寸。`draw` 使用公开 `Window::draw` 并释放绘制 arena，不发送输入；F12 会产生 VT 序列，模拟 Shift 也可能变成文本，均不可替代。键盘经 `window.defer` 分发，避免重入仍被借用的 Workbench；指针驱动不通过私有 GPUI `dispatch_event` 返回类型或依赖改造绕过接口。

概览 fixture 最多提供 61 条有界样例并创建未连接会话，进程/端口另有隔离采样入口；`preview` 在 UI 与后台都禁止信号。它们验证几何与状态处理，真实 Linux 采样/信号另行验收。系统页切换经 `set_active_tool`，`panel_width` 只证明偏好计算；组合输入动作只证明组合/提交/取消数据流。

报告记录源码提交或工作区差异、二进制与脚本指纹、环境、命令、预期/实际及证据类型。准备失败、驱动时序失败与产品失败分开记录；不累加不同构建的通过数。截图与状态来自同一已编译版本，锁屏不能截图时记为待补，不复用旧图。提交报告列出实际运行、未执行检查及剩余限制，平台状态统一维护在[验收说明](acceptance.md)。

## 平台兼容点

`ui/macos_titlebar.rs` 处理 GPUI 0.2.2 全尺寸透明标题栏：AppKit/Window Server 可能在 mouse-down 到达 GPUI 前启动拖窗，该版 `on_hit_test_window_control` 与 macOS `start_window_move` 无法提供所需覆盖。应用自有 `MantaSHTitlebarExclusionView` 按可见控件的绘制边界放置透明原生区域，`hitTest:` 返回 nil 让输入穿透，`mouseDownCanMoveWindow` 返回 NO，`_opaqueRectForWindowMoveWhenInTitlebar` 返回 bounds，将控件区域从标题栏拖动区排除。空白处保留系统拖窗，`NSWindow.isMovable` 保持 true。

区域按标签滚动视口裁剪并转换坐标，稳定排序、复用，关闭时解除附着，持有 GPUIView 弱引用并有 RAII 兜底；仅几何变化时暂时切换并立即恢复 `movableByWindowBackground` 刷新 AppKit 缓存，两次设置间不分发事件。顶栏只转发一次 `titlebar_double_click`，避免重复 Zoom。这是未公开的 AppKit 兼容点，升级 macOS/GPUI 后须复查；参考 [Firefox 原生区域机制](https://github.com/mozilla/gecko-dev/blob/8f0a87156f64f797a45e2773654538b0e99904d7/widget/cocoa/nsCocoaWindow.mm)和[新版 GPUI 标题栏处理](https://github.com/zed-industries/zed/blob/72c53bf0aae2e619f146d96415f79caf77310455/crates/gpui_macos/src/window.rs)，实现不修改系统或 GPUI 类。

`install_system_menu` 显式注册本地化 `NSApplication.windowsMenu`，使用 `performMiniaturize:` / `performZoom:`；系统菜单循环在释放 GPUI 更新借用后进入。About 也先经 `App::defer`，再打开共享 `Modal::About`，避免同名父帧重复入栈。macOS 日常窗口入口使用系统菜单，直接几何接口保留给其他平台与专项回归。

Windows 标题栏空白拖动达到阈值后释放捕获并投递 `WM_SYSCOMMAND(SC_MOVE | HTCAPTION)`；三枚系统按钮映射最小化、最大化/还原和关闭，并注册 `WindowControlArea`。关闭仍走草稿和传输保护；这些实现接入不代表 Windows 实机验收完成，检查入口见 [Windows 指南](windows.md)。

## 依赖与本地补丁

新增或升级依赖先核对功能必要性、平台支持、最低 Rust 和许可证，更新锁文件、[第三方说明](../THIRD_PARTY.md)及 [dependency-licenses.csv](dependency-licenses.csv)。[dependency_licenses.py](../scripts/dependency_licenses.py) 提供清单生成入口。SSH 使用 `russh >= 0.63.2`，Windows 间接 `pageant` 锁定 0.2.3；安全修复要求 Rust 1.89，正式产品不启动 SSH 服务端或连接 Pageant 代理。

`Cargo.toml` 的 `[patch.crates-io]` 有两项本地补丁，升级前阅读其说明并验证相关调用：

| 补丁 | 范围与移除条件 |
|---|---|
| [taffy-compat](../vendor/taffy-compat/README.md) | 保持 GPUI 所需 0.9.0 包身份，转导出固定提交的 Taffy 0.9.2，采用修复的 grid 1.x；GPUI 接受修复版后移除 |
| [gpui-component](../vendor/gpui-component/MANTASH-PATCH.md) | 0.5.1 的显式编辑事务、选区/滚动恢复、撤销分组，以及按钮按 variant 使用 hover 前景；上游提供等价 API 后移除对应补丁，保留说明与许可证 |

Logo 几何与配色统一维护在 [generate_logo.py](../scripts/generate_logo.py)，由它生成 assets 中的 SVG、PNG、ICO 和 ICNS；资源规范及复现检查见[设计规范](design.md#品牌资源)。本地验包用 [package_release.py](../scripts/package_release.py)，DMG 验证用 [verify_macos_dmg.py](../scripts/verify_macos_dmg.py)。版本、runner 构建、签名及 [release_version.py](../scripts/release_version.py) 等发布脚本统一见[发布文档](release.md)，不把打包或 Actions 构建当作目标平台人工通过。

开始修改前确认工作区已有改动与授权范围；代码沿用架构边界，新增文案进入 `src/ui/i18n.rs`，控件走统一包装层及 `theme::SPACE_*`。按功能维护文档和提交要求见[贡献指南](../CONTRIBUTING.md)，自动化协作者遵守 [AGENTS.md](../AGENTS.md)。
