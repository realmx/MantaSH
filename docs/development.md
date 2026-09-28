# 开发与验证

面向维护者的代码组织、实现约定与可执行验证入口。视觉与交互规格见[设计规范](design.md)，架构与数据流见[架构文档](architecture.md)，数据格式见[数据文档](data.md)，发布流程见[发布文档](release.md)。

## 代码组织

- `model.rs` / `storage.rs`：可持久化元数据与 SQLite 读写、损坏恢复。
- `connections.rs`：连接 CSV 导入导出与合并；`encoding.rs`：严格编码转换、BOM 与二进制判定。
- `terminal.rs`：ANSI 网格、选区与输入编码；`titles.rs`：可靠标题；`integration.rs`：会话内 Shell 事件注入并恢复用户 HISTFILE。
- `ssh.rs`：主机验证与传输建立；`files.rs`：SFTP 与文件传输；`monitor.rs`：Linux 采样解析；`processes.rs`：进程身份与信号。
- `layout.rs`：布局树与右栏宽度；`services.rs` / `events.rs`：后台任务调度与类型化事件。
- `file_selection.rs`：文件选区（完整路径、范围起点、键盘终点）。隐藏过滤同步清除不可见目标；原生行事件固定 Owner 与目录请求 ID，拒绝过期请求、其它窗格和已切换工具页的事件；批量确认按当前可见顺序快照目标。
- `ui/`：GPUI 视图、交互、颜色与文案；新增界面文案进入 `src/ui/i18n.rs`。概览模块分隔线用 `border_t_1 + border_color` 保证全宽绘制。

桌面 feature 默认启用；`--no-default-features` 保留业务与真实协议服务，适合无窗口测试。正常 UI 无模拟适配器，测试服务器只存在于 tests。

## 实现约定

- 渲染函数不执行网络、文件或数据库 IO；后台失败保留原目标与可恢复状态，不依赖当前标签应用结果。不用 `unwrap` 处理用户输入或远程错误；测试断言与不可恢复的初始化错误分别说明。
- 异步事件必须携带 Owner（session UUID + attempt UUID）；目录导航、监控采样、进程详情、传输另有独立请求 UUID。消费端复核归属，丢弃过期、重连、取消、切换标签或工具页及其它窗格的旧结果。
- 间距只用 `theme::SPACE_*` 阶梯（见[设计规范](design.md#排版与组件)）。可点击控件统一走 `ui/controls.rs` 按钮包装层：悬停手型、禁用禁止光标；tooltip 只保留在按钮上，概览卡片、趋势图、历史文本等非按钮元素不注册。
- 修改 UI 文案必须更新 `i18n.rs` 集中目录，并同步用户手册中的入口与快捷键。

## 必做检查

```sh
cargo fmt --all -- --check
cargo test --locked --no-default-features --all-targets
cargo check --locked --no-default-features --all-targets
python3 scripts/check_docs.py
```

只扩展与变更相关的验证，不用重复测试制造通过数量。修复事件归属、取消、编码、导入或保存逻辑时必须附带能区分正确/错误行为的测试；简单文案和尺寸修改以原生检查为主。跨模块修改时分别运行相关功能、原生 QA 和平台检查，统一记录当前源码与脚本指纹；测试准备失败或驱动时序问题单独记录，不把不同构建的结果累加。

## 弹窗系统

`Workbench::modal` 保存当前弹窗；`modal_stack` 保存被普通子弹窗暂时覆盖的 `ModalFrame`（弹窗实例、滚动句柄、弹窗焦点和打开前的具体控件焦点），`modal_confirm_return` 单独保存有既定业务去向的确认父帧。`show_modal` 在普通弹窗之间移动父实例，`dismiss` 恢复最近父级并重新聚焦可继续操作的输入或列表；搜索词、表单实体、选区和专用滚动句柄不因重建弹窗丢失。删除连接/文件/历史、主机信任、传输开始/停止、进程结束、冲突和未保存关闭等确认类型不进入普通父级栈，沿用固定目标校验、取消与成功后去向；确认链路结束会清理遗留父帧，避免下次独立打开时错误恢复旧弹窗。

- 遮罩必须 `.inset_0()` 占满顶层 `.relative().size_full()` 根节点后再 flex 居中，仅 `.absolute()` 会让遮罩缩到弹窗内容尺寸。
- 宽度经 `Modal::is_confirmation()` 分类设上限：确认类 480px、其它普通弹窗 640px、`Modal::Editor` 豁免保留 960px，另受窗口左右 16px 边距约束。
- 虚拟列表（连接库、文件树）遵循 gpui 0.2 `uniform_list` 的 `ListSizingBehavior::Auto` 约束：父容器必须经确定高度的 flex 链给定视口，条目根布局显式 `.w_full()`，否则选中/悬停背景按 fit-content 收缩。文件树行固定高度并共享显式宽度，列表设 `ListHorizontalSizingBehavior::Unconstrained`，同一 `UniformListScrollHandle` 承接两轴；目录定位用 `scroll_to_item`，不能依赖只绘制可见行的 paint 坐标。
- 历史弹窗高度按实际滚动视口测量内容，仅在 600px 或视口上限截断时由列表滚动；高度调整后的 QA 必须再执行无输入 `draw`，待下一布局帧更新 `ScrollHandle` 视口，不能把设置的目标高度当作已完成布局。
- QA：`scripts/qa_modal_widths_macos.py` 断言各弹窗外框与视口中心两向误差 ≤1px 及缩放后重新居中；`scripts/qa_history_connection_modals_macos.py` 覆盖历史与连接库弹窗的隔离回归，fixture 不启动传输、不写真实目录。

## 连接库

- 搜索由 `connections::matches_query` 匹配名称与 endpoint，空格分词、忽略大小写。键盘目标每次从当前可见结果按 UUID 解析，空列表不打开连接；Enter 经单行 Input 的公开 action 路由。搜索框仅捕获无修饰键 ↑/↓，输入法 marked range 期间交给输入法。快捷键与菜单共用 `OpenConnections`。
- 列表以 `uniform_list` 虚拟化：每帧只构建可见行；过滤行快照按查询词与全部资料字段指纹缓存在 `connection_rows_cache`，滚动帧不重复过滤或分配，资料变化整体重建。
- 多选状态保存在 `connection_multi`（UUID 集合）与 `connection_anchor`（范围起点）：单击单选并更新锚点，Cmd/Ctrl+单击增减，Shift+单击按可见顺序取范围（配合 Cmd/Ctrl 为追加范围），键盘 ↑/↓ 同步锚点。搜索词变化时集合与锚点裁剪到仍可见的行，隐藏行不能成为批量删除目标。批量删除确认后 `save_profiles` 持久化并在后台逐项 `vault.forget`；Escape 有选中行时只清空多选、锚点和高亮并保持弹窗打开。
- 行内编辑、克隆、连接图标按钮按 UUID 复核实时资料并阻断行级点击冒泡；连接动作始终 `open_saved(new_instance=true)` 新开标签，同一资料可开多个独立会话。
- 弹窗布局、footer 与表单排版见[设计规范](design.md#连接库)。调试驱动只暴露公开连接 ID、查询、布局、多选集合和表单状态，不暴露 secret 输入。

## 文件面板与传输

文件列表右键菜单仅保留文件操作，刷新在工具栏；删除项经组件库 `menu_element_with_disabled` 按启用状态使用错误色或禁用灰色，动作由 `FileMenuDelete` 复核目标。传输记录选区保存在 `transfer_selected`/`transfer_anchor`，Esc 有选中时只清空；`remove_transfer_records` 点击时重新核对弹窗 Owner、当前会话、选区与终态——有选区只移除所选已结束记录，否则移除该会话全部已结束记录，运行中不删除，删除后留在原弹窗并异步移除 SQLite 记录，不生成确认弹窗。

`Modal::Transfer` 显式保存批次 UUID、`TransferPhase`（Review/Running/Result）、记录 UUID、覆盖选项与核对时的原目录/请求快照。`confirm_transfers` 在注册 worker 前同步切到 Running，空任务、重复提交、过期 Owner/目录请求均不启动；选择器回调另核对活动会话、连接、路径、请求和下载选区。后台结果按 UUID 归属收敛：只有全部成功才自动关闭（且仅当原 Owner、原路径与请求仍一致时刷新文件列表，用户手动导航后不回跳），失败/取消进入 Result 并保留逐项原因。标题栏 X、Esc 和“后台继续”只关闭进行中视图；停止由 `Modal::CancelTransfers` 固定主机及当时活跃任务 UUID，确认时再次核对归属，只请求后端停止，终态以 worker 回复为准。失败/取消结果提供逐项重试：只在新 UUID 上重新展示核对页，不覆盖旧历史。并发上限两个 worker，临时文件保护不变；`transfer_paths::endpoints` 与 `display_name` 只负责展示，不参与路径计算。用户可见的阶段排版与进度呈现见[设计规范](design.md#文件面板与传输)。

QA：`scripts/qa_transfers_macos.py --directory <目录> --fixture <fixture 目录>` 经真实回环 SFTP 验证多文件/目录传输、字节哈希、目标绑定、覆盖失败、重试、并发限制和取消；启动 QA 应用时把 `MANTASH_QA_REMOTE_ROOT` 设为 fixture.json 中 root 经真实路径解析后的值（macOS `/var` 与 `/private/var` 是同一路径的不同写法）。`stage_transfer` 只替代系统文件选择器，确认、取消和重试与正式界面共用入口；原生确认及服务真实执行不代表系统文件选择对话框已通过物理鼠标验收。

## 系统工具：进程与端口

进程列表（`Modal::SystemTools { page: Processes }`）、单进程详情（`Modal::ProcessDetails`）与端口列表（`page: Ports`）是三个独立弹窗，均读取所属 SSH Pane 的监控采样：连接成功后开始采样，可见期间每 3 秒刷新，无手动刷新按钮；筛选、排序、展开与滚动状态属于 Pane，重连/断线清理。

- `processes::current_process` 要求有效 Linux 进程段、原 boot ID、原 PID 和启动 ticks 完全一致；`process_target` 另核对连接、采样错误与 10 秒新鲜度。详情指标只读该返回值，禁止从已失效旧快照或同 PID 新实例补值；`process_action_reason` 与按钮禁用、提交前保护共用判定。
- `Modal::ProcessDetails` 保存固定 Owner/进程身份、最新详情请求 UUID、旧读取结果与展开位；`apply_process_details_result` 只接受 Owner 与最新 UUID 匹配且仍在等待的回复（含确认页暂存的父弹窗），失败时保留上次成功命令。长命令限高预览的复制按钮始终复制完整原文，原始状态展开后独立限高滚动，不增加后端请求。`ProcessConfirm` 固定主机与动作，取消和确认提交后均恢复原详情；发送状态驻留 `process_attempts` 按请求 ID 接受异步结果。`Outcome::Sent` 不等于 Gone，后续有效采样才确认原实例退出，10 秒未确认显示 StillRunning，既不自动升级 SIGKILL 也不重试。
- 端口数据保持 `ss` 原始记录语义：`port_view.rs` 只做端点拆分、完整字段筛选、协议归类、数字排序与重复行 `PortKey`；未加方括号的 IPv6 不猜端口，UDP 状态按原样显示。`port_view::process_pids` 从 `ss -p` 提取去重 PID，展开区进程详情复用 `show_process_details`，仍经 Owner、身份、boot ID、PID 和启动 ticks 核验。复制前再次核对 Owner、当前弹窗和可见采样，不执行 SSH 写操作。
- 列表几何与排版契约见[设计规范](design.md#系统工具)。QA：`qa_process_list_macos.py`（0/3/90 行、表头行对齐、筛选稳定高度）、`qa_process_details_macos.py`（过期回复、两种确认取消、身份失效、长命令与原始状态）、`qa_ports_macos.py`（0/3/5/48 行、表头行几何对齐、展开/搜索/排序/复制、错误与断线）均使用隔离样本，不把 QA 绘制当成正式入口真实鼠标验收。显式 QA `preview` 双重禁止信号，合成采样/命令不连接远端；真实信号路径由 `processes.rs` 测试与后台 SSH 请求覆盖。

## 系统概览

`monitor_history.rs` 维护有限采样窗口与缺失数据分段，GPUI canvas 绘制网络趋势（限 56px）；CPU、内存和磁盘显示当前百分比。`Palette::meter_color` 对 0–25% 与 85–100% 做 sRGB 插值、中间固定蓝色；`resource_meter` 共用 0–100% 轨道，`network_card` 是概览中唯一绘制 `trend_chart` 的视图。配色、模块布局与信息密度见[设计规范](design.md#系统工具)。

- 资源入口为 GPUI 可聚焦行：`show_resource_details` 验证当前 Owner 和概览页面后打开 CPU/磁盘详情；`MantaSHResource` 上下文绑定 Enter/Space，FocusHandle 跟随 Pane 保存，关闭后返回原焦点句柄。资源弹窗固定所属 Owner，无主机/采样时间栏与底部操作栏，正文随最新概览样本更新。
- `Pane.pending_terminal` 等待本次连接成功，失败/取消不替换旧显示终端；新 Owner 的旧异步结果继续由原过滤入口拒绝。历史操作只有可验证的 Shell 提示符才能填入/执行；文件行操作在提交前重新验证 Owner、request 和路径。
- QA：`scripts/qa_overview_layout_macos.py --binary <debug 可执行文件> --directory <全新临时目录>` 用 `overview_fixture`（最多 61 条有界 Linux 样例，拒绝进程/端口记录，生成未连接会话）执行 draw 后测量多种视口/字号的模块高度、全宽分隔线、弹窗内边距和滚动边界，不连接 Linux、不发送键盘输入。`scripts/check_overview_pixels.py --image <PNG> --state <state.json>` 对已呈现窗口的截图按 ICC 转换 sRGB 后核对轨道填充比例与网络曲线；截图和状态必须来自同一已成功编译的版本，不用 draw 或其它二进制截图代替正常呈现。

## 本地终端与 PTY

输出唤醒、damage 复制与刷新调度见[终端机制](terminal-reference.md)。本地实现要点：

- Unix 本地 PTY 使用单一非阻塞 I/O 循环：先排空当前宽度的输出，以 500ms 稳定窗口等待 Zoom 中间尺寸结束，最终排空并重新检查队列后才更新 PTY 与仿真器网格；Windows 保留 ConPTY 既有后台路径。提示符和右侧提示始终由 Shell 的真实 SIGWINCH 重绘负责，应用不向 emulator 注入合成滚屏或提示符移动序列。
- `TerminalBuffer` 用 VTE 状态解析器跟踪标准 `clear` 输出的 `CSI 3 J`、光标归零和 `CSI 2 J`：alacritty 的 `2J` 会先把旧视口行移入 scrollback，因此在该组合的 `2J` 字节边界完成后再次清理历史；普通 `2J`、单独 `3J` 和有新文本隔开的控制序列不触发补清。主 ANSI parser 按块处理，跨 PTY read 的转义序列由跟踪器保留状态。
- `resize_local` 只在本地、可信提示符未收到输入、光标在首行、原 history 为空且其余可见行全空时，清除本次 resize 刚产生的 history（未清屏 zsh RPROMPT 的 reflow 场景）；有真实 scrollback、编辑中的命令或下方输出时保持原网格语义。输入状态只保存两个布尔值，提交后的新提示符才复位。
- `TerminalView` 只接受与测量尺寸相同的网格帧，并在每个布局帧清空 retained canvas，避免 AppKit 动画期间的旧字形残留。

## 标签栏与 macOS 标题栏

- `ui/tab_reorder.rs` 在顶栏内记录手势 UUID、源标签 ID、水平位移和候选插入位置。标签命中区域用 occlude 隔离 GPUI 事件，窗口级捕获监听处理移动与释放；横向位移达 4px 才进入排序，按可见标签中点计算插入位置，栏外释放和 Esc/失焦取消。滚轮事件显式转交标签 ScrollHandle，边缘滚动随手势结束停止；排序后按 UUID 恢复活动索引，不将排序误当成会话切换触发自动滚回。关闭按钮有独立命中区域。
- 原生 `TitleBar` 的内部行禁止收缩，`min_w_0` 无法限制其最小内容宽度：标题栏内容在分配区域内定位，避免标签总宽度参与祖先最小尺寸计算；活动标签仅在首次显示与尺寸变化后的测量完成后补一次定位，不在每次输出重绘时重复。
- 标签底色由父容器管理，`TabLabel` 使用透明的 GPUI 自定义按钮配色防止组件库 ghost 悬浮背景叠加在选中标签上；`TabClose` 单独提供中性局部反馈，三项专用语义色不改通用选中色。标签状态不改变字重或宽度。
- macOS 全尺寸透明标题栏下，Window Server 按 AppKit 提前注册的区域拖动窗口，可能不向应用发送 mouse-down；GPUI 0.2.2 的 `on_hit_test_window_control` 未实现、`start_window_move` 无 macOS 覆盖，occlude、停止冒泡和 app-local 事件监视都不足以修复。`ui/macos_titlebar.rs` 用应用自有 `MantaSHTitlebarExclusionView` 在每个可见标签、按钮和应用图标的实际位置放置透明原生区域视图：`hitTest:` 返回 nil（鼠标、滚轮和焦点继续进入原 GPUIView）、`mouseDownCanMoveWindow` 返回 NO、`_opaqueRectForWindowMoveWhenInTitlebar` 返回自身 bounds，把该矩形从 Window Server 的标题栏拖动区域排除；空白处不放区域视图，保留系统拖窗、交通灯和窗口菜单，`NSWindow.isMovable` 保持 true。区域按绘制后的标签滚动视口裁剪换算到 NSView 坐标，按位置稳定排序并复用未改变视图；关闭、滚动、字号和窗口尺寸变化时更新/移除，仅几何改变时通过暂时切换并立即恢复 `movableByWindowBackground` 使 AppKit 刷新缓存（两次设置间不分发事件）。区域持有 GPUIView 弱引用，关闭解除附着，RAII 兜底。该 AppKit 兼容点未公开，参考 [Firefox 的原生区域机制](https://github.com/mozilla/gecko-dev/blob/8f0a87156f64f797a45e2773654538b0e99904d7/widget/cocoa/nsCocoaWindow.mm)与[新版 GPUI 的标题栏处理](https://github.com/zed-industries/zed/blob/72c53bf0aae2e619f146d96415f79caf77310455/crates/gpui_macos/src/window.rs)独立实现，不复制业务视图、不修改系统或 GPUI 类、不升级锁定依赖；升级 macOS/GPUI 后必须复查。
- 顶栏 Stateful header 只转发一次 macOS `titlebar_double_click` 并阻止外层 GPUI TitleBar 重复转发，空白区域双击只执行一次 Zoom。
- QA：`scripts/qa_tab_reorder_macos.py` 在投递程序化 NSEvent 之前等待 AppKit 自己查询当前原生区域（调试计数在几何改变时归零），比较原生区域与 GPUI 绘制边界，验证空白不被遮挡、事件穿透、窗口仍可移动及排序、取消、关闭、滚动、字号与工作区保存；能发现缺失原生区域处理的回归，但仍不等同于真实物理鼠标验收。

## 窗口菜单与 About

- macOS `install_system_menu` 显式注册本地化的 NSApplication.windowsMenu，并使用标准 performMiniaturize:/performZoom: responder action（锁定版 GPUI 只会自动注册字面名称为 Window 的菜单）。顶栏/快捷键经保留的 NSMenu 和所属 NSView 打开真正的系统菜单，在前台执行器中释放 GPUI 更新借用后进入 AppKit 菜单循环；系统负责平铺、居中、全屏与还原选项。
- `window_layout.rs`、尺寸表单和直接几何接口保留给其它平台与几何回归；macOS 日常菜单使用系统 WindowMenuProbe 和菜单只读快照。Windows 适配未实机验证，Linux 不新增本地 VM/容器。
- 应用菜单以本地化的“关于 MantaSH / About MantaSH”为首项，接分隔线、“设置”、Services 和退出。`OpenAbout` 在释放 GPUI 更新借用后调用系统 UI：macOS 使用 `orderFrontStandardAboutPanelWithOptions:` 传入应用名和 `APP_VERSION`（已打包 `.app` 的图标来自 `Info.plist`）；Windows 使用绑定当前窗口 HWND 的 ShellAboutW。About 是系统标准面板，不进入 Workbench 的弹窗栈；设置页仍显示版本。`scripts/qa_about_menu_macos.py --binary target/debug/mantash --directory <全新隔离目录>` 在隔离原生窗口检查真实 AppKit 菜单、独立系统面板、版本文本和设置弹窗保持状态，不代替人工物理点击或 Windows 实机验证。

## 共享命令历史

`HistoryScope` 将 local 与 `ssh:<profile UUID>` 记录映射为本地与共享 SSH 两份逻辑列表，保留存储中的源 UUID，不改写已有记录。`ui/history.rs` 的 HistoryView 在 Workbench 中按范围持有搜索、滚动和多选选区，SSH 标签不各自持有。历史来源不决定执行位置：行操作捕获当前 Owner，直接执行再次验证 attempt 与可靠 Shell 提示符后，把命令粘贴并提交到该终端。弹窗排版与选择语义见[设计规范](design.md#共享-ssh-历史)。

`tests/shared_history.rs` 验证跨来源 SSH 记录重启后合并为同一共享列表、时间顺序、本地隔离与按 UUID 精确删除；local 记录与畸形来源不进入共享列表。原生驱动 HistoryFixture 创建断线视图后直接打开历史弹窗，HistoryQuery/HistoryScroll 验证共享浏览状态，不连接 SSH。

## 本地自动凭据记忆

`vault.rs` 实现无版本号的单一凭据格式（魔数 `MantaSH local credentials` 标识）：系统随机源生成的本机 256 位密钥与 [RustCrypto AES-GCM](https://docs.rs/aes-gcm/0.11.1/aes_gcm/)，每次写入独立 nonce；key 文件由 tempfile 的 `persist_noclobber` 发布，已存在时复用，不生成替代 key。数据文件与保护范围见[数据文档](data.md#凭据)。

- `Backend` 持有 LocalVault，所有文件、数据库和加密操作在后台线程执行。正常 SSH 流程只在主机验证后读取凭据，认证成功后自动写入；无主密码、不调用系统钥匙串、无“记住密码”选项，错误密码不覆盖已保存值。
- 编辑已保存连接时 `profile_form` 生成新的请求 UUID 并在后台调用 `SecretStore::read`；结果仅在请求仍对应当前表单、认证方式未改变且用户输入仍为空时填入可见密码字段。QA 只导出 `secret_present`/`secret_loading` 布尔状态，不导出密码内容；克隆路径使用 `profile_form_without_secret`。
- `tests/automatic_vault.rs` 验证自动保存与重启读取、并发首次保存、key 丢失/损坏、nonce、UUID 绑定与按 UUID 删除；`tests/support/credential_reconnect.rs` 验证主机确认前不读凭据、认证成功后保存并复用、拒绝后手动更正、取消旧回复和凭据库失败。`scripts/qa_credentials_macos.py run` 使用隔离的本地加密密码库完成密码保存、重连、新开实例及实际重启后的直接复用。

## 原生视图驱动（debug QA）

仅 debug 构建在同时显式设置 `MANTASH_QA_CONTROL` 与同一目录下的 `MANTASH_DATA_DIR` 时启用；正常启动不读取控制文件。驱动使用真实视图动作和真实后端，不绕过生产主机验证，只允许为回环测试连接填入资料。驱动启动时忽略控制文件中上一次的请求，避免重放 Type 或 Quit；`state.json` 包含隔离会话的可见终端输出，只用于测试数据，不能提交真实用户内容。

```sh
MANTASH_DATA_DIR="$TMPDIR/mantash-ui-qa/data" \
MANTASH_QA_CONTROL="$TMPDIR/mantash-ui-qa/command.json" \
cargo run --locked
```

另一个终端中：

```sh
python3 scripts/qa_control.py settings
python3 scripts/qa_control.py font_sizes --data '{"ui":18,"terminal":17}'
python3 scripts/qa_control.py resize --data '{"width":960,"height":640}'
python3 scripts/qa_control.py scroll_modal --data '{"y":-500}'
python3 scripts/qa_control.py keystroke --data '{"key":"tab"}'
python3 scripts/qa_control.py theme --data '{"night":true}'
python3 scripts/qa_control.py dismiss
```

另有 `focus_pane、close_pane、panel_width、hide_tool、reopen_tool、system_page、refresh_monitor、reconnect、local/split/profile/submit_profile/trust` 等定向动作。

- 命令确认只表示动作已接收。异步连接、读取或保存必须继续检查 `state.json` 直到期望状态出现；窗口调整后等实际尺寸稳定再截图。`MANTASH_QA_BACKGROUND=1` 令 QA 窗口不主动激活，`MANTASH_QA_HIDDEN=1` 进一步隐藏窗口（仍运行原生键盘分发和真实 PTY，不用于视觉截图验收）。
- 键盘使用公开且返回 bool 的 `Window::dispatch_keystroke`，经 `window.defer` 延迟分发避免仍持有 Workbench 更新借用时回调同一视图；`keystroke` 经过原生焦点树、键绑定和输入事件，不能用 `type` 写入制表符代替键盘回归。鼠标不注入：GPUI 原始 `dispatch_event` 返回私有类型，驱动不通过修改依赖或不安全代码绕过；`pointer_gesture` 只把 AppKit 事件投递给隔离测试窗口的明确窗口号。物理键鼠和系统输入法界面单独验收。
- 隐藏窗口做尺寸检查时，在更改字号/主题后用 `draw` 请求原生绘制（调用公开 `Window::draw` 并释放绘制 arena，不发送输入；F12 会发送 VT 序列、模拟 `shift` 可能被转成文本，均不可代替），等待实际边界更新后再比较；布局检查需完成两次 `draw`，在前一轮定位完成前设置滚动偏移会把测试时序混入用户滚动场景。原生截图做像素比较前须按内嵌 ICC 色彩配置转换到 sRGB；macOS 锁屏时记录截图待补，不将旧图改名冒充。
- 测试驱动的系统页切换必须经 `set_active_tool`，与真实按钮共用 last_tool 记忆；`panel_width` 只检查偏好计算，`resource_details` 等场景构造动作不代表物理点击；组合输入动作只证明组合/提交/取消的数据流，不证明系统候选框。

长期原生验收服务器（唯一被 ignore 的测试是测试工具，不是未通过而被忽略的产品测试）：

```sh
MANTASH_FIXTURE_DIR="$TMPDIR/mantash-ui-qa" \
cargo test --locked --no-default-features --test ssh_sftp native_qa_fixture -- --ignored --nocapture
```

提供随机口令/密钥与临时目录，最长 45 分钟，可在指定目录创建 `stop-fixture` 文件提前停止；期间不得连接生产主机。

## 原生 QA 脚本索引

除特别说明外，脚本均接受 `--directory <全新隔离 QA 目录>`；需要回环服务器的另加 `--fixture <fixture 目录>`（由上一节的 native_qa_fixture 启动）。

| 脚本 | 覆盖 |
|---|---|
| `qa_smoke.py` | 已运行隔离窗口的冒烟：第一个标签 5 个本地窗格，第二个为 fixture 创建并信任的 SSH 会话（先用 `qa_control.py local/split/profile/submit_profile/trust` 建场景）；输出 `smoke-results.json`，须核对异步结果后才记为通过 |
| `qa_accept_macos.py` | exercise / restore 两阶段，按阶段实际退出并重启原生程序：组合输入适配器、五窗格、右栏、在线编辑重新读取最新内容、最后保存覆盖和工作恢复 |
| `qa_completion_macos.py` | 文件、中文文件名、目录、命令名称补全，前台程序 Backtab、组合输入保护及表单导航；使用真实本地 Shell，不需要 fixture |
| `qa_tabs_macos.py` | 顶部标签布局回归：动态名称、滚动边界、后台输出、活动标签显示与窗口尺寸；`state.json` 的 `header` 来自原生预绘制 |
| `qa_tab_reorder_macos.py` | 标签水平排序与 macOS 原生标题栏拖动区域（见上文） |
| `qa_file_rows_macos.py` | 真实 SFTP 的单选、范围/增量选择、隐藏过滤、双击导航、键盘操作和批量确认快照；`file_row` 参数含 session、attempt、request |
| `qa_transfers_macos.py` | 传输后端与确认/取消/重试入口（见上文，另需 `MANTASH_QA_REMOTE_ROOT`） |
| `qa_modal_widths_macos.py` | 各弹窗外框宽度上限与居中误差 |
| `qa_history_connection_modals_macos.py` | 历史与连接库弹窗隔离回归 |
| `qa_process_list_macos.py` / `qa_process_details_macos.py` | 进程列表几何与详情身份/确认/过期回复 |
| `qa_ports_macos.py` | 端口列表几何、筛选、排序、展开与复制（`--binary target/debug/mantash`） |
| `qa_overview_layout_macos.py` | 概览布局测量（`--binary <debug 可执行文件>`） |
| `qa_connection_duplicate_macos.py` | 同一 fixture 资料经真实 Enter 连续打开两次，断言两个独立标签（回环无监听端口，会话自行失败） |
| `qa_connection_row_width_macos.py` | 连接行宽度等于列表容器宽度、弹窗高度上限 600px、过滤后仍占满 |
| `qa_credentials_macos.py run` | 隔离密码库的保存、重连、新开实例与重启复用 |
| `qa_about_menu_macos.py` | AppKit 菜单、系统 About 面板与设置弹窗状态（`--binary target/debug/mantash`） |
| `qa_stress.py run --seconds 600` | 混合负载：五窗格输出外每五轮执行 SFTP 刷新、编辑保存和上传；窗口保持可见、不与其它基准并行 |
| `qa_bundle_macos.py` | 搬离源码目录后的 Launch Services 启动和凭据访问；不执行发布打包或签名 |
| `check_overview_pixels.py` | 截图像素核对（`--image <PNG> --state <state.json>`，ICC→sRGB） |

快照断言用字段：`profile_order`（连接库保存顺序名称列表，配合 `pointer_gesture` 真实指针路径验证拖拽排序）、`file_tree_loading`/`file_tree_rows`（树展开加载与完成）、`file_input`/`file_input_focused`/`file_input_bounds`（路径输入框与当前路径同步）；`file_path_input` 动作配合 `keystroke` 走真实键盘分发验证回车语义。原生文件选择状态由 GPUI 真实视图驱动验证：选择时只更新既有行的高亮和固定操作控件，不重建行或图标；真实鼠标单击、Shift、双击和系统文件选择器仍按平台分别验收。

## 自动化覆盖

- core：CSV 导入导出与合并、凭据字段隔离、编码/BOM、二进制拒绝、跨包多字节、ANSI 选区搜索、控制键、历史 nonce、SQLite 损坏保留、焦点恢复、Linux 采样差值、路径和终态。
- shared_history：跨来源 SSH 记录重启后合并为同一共享列表、按 UUID 精确删除、同一命令折叠为最新一条；local 记录与畸形来源不进入共享列表。
- local_pty：真实 OS Shell 的输入、输出、连续 resize 与关闭（Unix 非阻塞循环见上文）。
- ssh_sftp：仅监听回环地址的临时 SSH 服务器，验证指纹和密码认证顺序；SFTP 请求桥接系统 OpenSSH 的 `sftp-server`，读写真实临时文件，覆盖外部修改后重读、最后保存覆盖、权限失败、目录传输、符号链接拒绝、取消和原内容保留。测试口令随机生成，不输出内容。
- terminal_updates：合并通知在多线程输出下保持一次唤醒、消费后重新唤醒，增量帧与完整帧逐格一致，单行更新复制量、触控板累计及修饰功能键；真实 PTY 场景验证输出通知不依赖 UI 定时轮询。
- layout_storage：最多五个叶子、关闭不丢失、损坏图恢复、偏好宽度、无标记文件采纳格式标记与其它标记拒绝。
- processes：采样前后身份一致、保护 PID、编码详情、信号枚举、未知结果以及 macOS 拒绝 Linux 操作；Linux 条件测试只创建自己的 `sleep 30` 子进程验证失效身份拒绝、TERM/KILL 和实际退出，不操作生产进程（当前开发机不运行该条件测试）。

正常原生窗口、无输入 `draw`、程序化键盘、指针驱动和平台人工验收是不同证据，不能互相冒充；测试请求被接受不代表异步操作或布局已经完成。

## 依赖、资源与发布

- 保留锁文件。新增依赖前检查平台支持、许可证与功能必要性，并同步第三方清单（`docs/dependency-licenses.csv`）。
- LOGO 源几何在 assets SVG，`scripts/generate_logo.py`（Pillow）按同一 64 单位几何生成各尺寸 PNG、ICO 和 ICNS；只是资源生成，不是应用打包。品牌规范见[设计规范](design.md#品牌资源)。
- 本地开发与调试运行 `cargo run --locked`（[macOS 指南](macos.md)、[Windows 指南](windows.md)）；`scripts/package_release.py` 可在对应平台配合已构建二进制做本地验包，正式公开发布由 GitHub runner 完成。公开版本使用 `scripts/release_version.py` 从 `Cargo.toml` 基准版本和既有 tag 计算下一个 `vX.Y.Z` 并在构建前验证输入；`release.yml` 在 runner 上临时把 `Cargo.toml`、`Cargo.lock` 和 `docs/dependency-licenses.csv` 同步为 tag 版本后编译，主分支不产生版本提交。完整触发方式和产物边界见[发布文档](release.md)；GitHub Actions 交叉编译通过不等于目标平台人工验收通过。
- 提交遵守[贡献指南](../CONTRIBUTING.md)和 [AI 规范](../AGENTS.md)。
