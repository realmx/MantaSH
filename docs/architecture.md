# 架构与数据流

MantaSH 是原生 Rust/GPUI 桌面应用。GPUI 管理视图、焦点与绘制；后台服务负责 PTY、SSH/SFTP、文件、数据库和凭据操作，通过类型化事件回到界面。默认 `desktop` feature 提供桌面入口，`--no-default-features` 保留业务模型和真实协议服务。

本文说明模块、状态归属和关键实现约束。产品行为见[功能说明](features.md)与[用户手册](user-guide.md)，界面规格见[设计规范](design.md)，持久化格式见[数据文档](data.md)，构建和验证入口见[开发指南](development.md)。

## 模块与关键类型

下表路径均相对 `src/`。

| 职责 | 模块与关键类型 |
|---|---|
| 工作区与会话模型 | `model.rs`：`Owner`、`SessionSpec`、`Preferences`、`SavedTab`、`SavedPane`、`TransferRecord`；`layout.rs`：`PaneLayout` 与工具面板宽度计算 |
| 后台调度与事件 | `services.rs`：`Backend`、`Session`；`events.rs`：`Event`；`services/metadata.rs`：`Mutation`、`Receipt` 与元数据 FIFO |
| 存储与连接资料 | `storage.rs`：`Database`；`storage/migration.rs`：备份与迁移；`connections.rs`：CSV 预览、合并、导出及密码写入重试 |
| SSH 与凭据 | `ssh.rs`：握手、主机验证、密码认证和通道；`credentials.rs`：`SecretStore`、一次性凭据回复；`vault.rs`：`LocalVault` |
| 终端 | `terminal.rs`：`TerminalBuffer` 与 ANSI 网格；`terminal_io.rs`：`OutputWakeup`、PTY 尺寸提交、输入增量；`terminal_paint.rs`：稀疏绘制数据；`ui/terminal_view.rs`：原生输入和网格视图 |
| Shell 元数据与历史 | `integration.rs`：Shell 集成；`titles.rs`：标题；`command_cursor.rs`：提示符定位；`model/history.rs`：`HistoryItem`、`HistoryCatalog`；`ui/history.rs`：固定提交目标 |
| 文件、编辑和传输 | `files.rs`：SFTP、`OpenIntent` 打开/预览、保护式保存、传输；`encoding.rs`：严格转码、文本允许列表、控制字符策略、BOM 与回退解码；`file_selection.rs`：路径选区；`editor_format.rs`：格式化及 UTF-8 选区映射；`ui/file_actions.rs`、`ui/editor.rs`、`ui/transfer_paths.rs`：操作快照、`Document` 预览/编辑交互和路径展示 |
| Linux 系统工具 | `monitor.rs`、`monitor_history.rs`：采样、差值、有限趋势窗口；`processes.rs`：`Identity`、`Action`、`Outcome`；`port_view.rs`：端口解析和排序；`ui/system.rs`、`ui/ports.rs`：所属会话视图 |
| 工作台与弹窗 | `ui/mod.rs`：`Workbench`、`Pane`、`Document`；`ui/shell.rs`、`ui/tools.rs`：布局；`ui/dialogs.rs`：`Modal`、`ModalFrame`；`ui/commits.rs`：`CommitState`；`ui/tab_reorder.rs`、`ui/connection_reorder.rs`：排序手势 |
| 统一控件与平台 | `ui/controls.rs`、`ui/theme.rs`、`ui/settings.rs`、`ui/i18n.rs`：控件、颜色、偏好和文案；`platform.rs`：路径与 Shell；`window_layout.rs`、`ui/window_actions.rs`、`ui/window_native.rs`、`ui/macos_titlebar.rs`、`ui/windows_caption.rs`：窗口适配 |
| 自动更新 | `update.rs`：官方源、资源选择、下载校验与安装助手；`ui/updates.rs`：检查、确认、取消和关闭交接 |

## 归属、请求与版本

界面保存运行会话的参数快照，后台结果始终回到发起操作时的对象。当前标签、列表位置和显示名称不能替代身份。

| 标识 | 约束 |
|---|---|
| `Owner { session, attempt }` | session UUID 标识会话；重连保留 session、更换 attempt。终端、SSH、文件和监控结果匹配完整 Owner，旧尝试的关闭或取消不能影响新连接 |
| request UUID | 同一 Owner 内区分目录导航、文件操作、监控采样、进程详情和提交；只有仍在等待的请求可更新对应状态 |
| revision | 文档 revision 区分编辑版本；元数据 revision 区分提交回执；终端网格 revision 用于绘制失效。这些计数各有作用域，不互相替代 |
| 固定目标快照 | 文件确认保存完整路径和目录请求，历史保存弹窗实例与目标 Owner，传输保存任务 UUID 和原连接，进程保存主机及实例身份；执行前再次校验 |

切换标签可以让原会话继续接收有效后台结果，但旧视图回调不得把动作改投当前会话。重连、关闭、取消、导航或切换工具页后，消费端分别复核请求和视图是否仍有效。隐藏文件过滤同步移除不可见选区，批量目标按确认时的可见顺序固定。

`services/metadata.rs` 用专用线程上的单一 FIFO 执行历史记录/删除、资料保存/排序/导入和工作区保存。每项操作读取当前数据库状态并提交，随后产生含 request、递增 revision 和结果的 `Receipt`；`Event::Metadata` 携带已持久化的共享快照，界面拒绝旧版本。资料事务和密码库写入分别反馈，删除资料后忘记密码失败、导入密码部分失败都不能伪装成完整成功。

`CommitState` 随当前弹窗及父帧保留提交 request、错误与密码重试项。共享资料的更新与后续导航、返回或连接动作分开：关闭、取消和切会话会使后续动作失效，隐藏父帧可接收回执，但不会抢焦点。导入文件选择回调也核对原 request/Owner。

## 连接生命周期

本地会话由 `portable-pty` 启动真实 Shell；Shell 发现和创建在 UI 线程外执行，偏好只作用于新建会话。SSH 使用 `russh`，每次连接固定资料及 `remember_credentials` 策略。新建资料默认端口 22、用户名 `root`；编辑资料不会改变已运行会话的快照，连接库每次连接都新开独立会话。

SSH 顺序为 TCP/SSH 握手、主机公钥验证、读取或请求密码、密码认证、PTY/Shell。新主机或指纹变化通过带 Owner 的一次性回复确认；关闭、取消或握手超时后，旧回复不能批准新的尝试。终端、SFTP 和监控使用同一 SSH transport 的独立通道，不共用标准输出；SFTP 首次使用时懒加载，失败可重试而不销毁终端。

凭据缺失、读取失败或认证拒绝产生 `Credentials` 事件并进入 `CredentialsRequired`。请求按 Owner 排队，用户切入所属窗格后再显示；拒绝后等待更正，不自动循环认证。新输入的密码仅在认证成功后后台保存，`CredentialStorage` 单独反馈保存失败，已成功认证的会话继续运行；使用已保存值成功连接不重复写入。`Pane.pending_terminal` 在本次连接成功后才替换显示终端，失败和取消保留原显示。SSH 原标签重连会复用既有 `TerminalBuffer`：关闭旧 attempt 后将主屏当前视口推入 scrollback，并把保留历史的网格移交给相同尺寸的新终端状态，重置 ANSI/解码器、模式、当前与保存光标、字符集、滚动区域和旧事件；后台 feed 在持有网格锁后再次复核取消，旧 attempt 等锁期间留下的字节不能写入重连后的网格。新 Session 因此仍可滚动查看重连前的输出；新建 SSH 标签仍从空网格开始。

## 终端与历史

### 输出唤醒与 damage 渲染

后台读到 PTY/SSH 字节后增量解码，交给 `alacritty_terminal` 更新网格，再发送所属 Owner 的输出唤醒。每个会话的 `OutputWakeup` 合并尚未消费的通知；隐藏标签继续解析输出，不因每个数据块重绘。唤醒标记与快照确认在网格锁下协调，避免确认上一帧时丢失新输出。隐藏标签再次显示会确认最新快照，之后输出可重新唤醒。

GUI 使用 `try_lock` 取得变化行和光标信息，释放锁后生成绘制数据，绘制不等待 PTY 解析锁。alacritty damage 让未变化行复用 `ShapedLine`；默认空白不逐格绘制，相邻同色背景和安全 ASCII 合并，中文和组合字符保持网格坐标。字体、尺寸、选区、搜索及滚动变化使相应缓存失效。字体测量按字体/字号缓存，以 `M` 宽度和实际字形高度计算网格。本地选区拖选期间（含 SSH 标签与本地分屏窗格）视图在窗口级跟踪指针；越过上/下边缘时由 40ms 定时器按 `terminal_io::drag_autoscroll_lines` 的距离分级滚动显示偏移，并把选区终点移到边缘行，松开、指针返回或滚动偏移不再变化即停止；鼠标报告路径不参与。Shift+按下时若已有可见选区，`TerminalBuffer::select_extend` 保留 alacritty 选区原锚点和选区类型（词选仍按词扩展），仅更新终点并经 `select_to` 的 `include_all` 包含两端，随后的拖动与边缘自动滚动沿用同一路径；无可见选区时回退到 `select_start`。

终端回显由输出事件驱动；250ms 的配置、监控和 QA 维护检查不决定回显时机，事件处理会分批让出主线程。GPU 底层帧和纹理管理由 GPUI 提供。触控板不足一行的滚动量累计后提交，并区分本地历史回滚、应用鼠标报告和备用屏方向键；滚动条仅在存在历史行时显示。

### 本地 PTY 尺寸与清屏

Unix 本地 PTY 使用单一非阻塞 I/O 循环：先排空旧宽度输出，等待 500ms 尺寸稳定窗口，最终排空并重新检查队列，再提交尺寸。Unix 和 Windows ConPTY 都经 `terminal_io::resize_local_pty` 先让 OS PTY 接受尺寸，成功后才调用 `resize_local` 更新网格；失败不改网格或清除历史。Windows 后台短暂退避重试，并优先处理输入、关闭和更新的尺寸。OS resize 时不持有终端网格锁。

提示符及右提示符由真实 Shell 的窗口尺寸响应重绘，应用不注入合成滚屏或提示符移动序列。`resize_local` 仅在本地可信提示符未收到输入、光标位于首行、原 history 为空且其余可见行全空时，移除本次提示符 reflow 产生的 history；真实 scrollback、编辑中的命令和下方输出保留原语义。`TerminalView` 只接受与测量尺寸相同的帧，每次布局清空 retained canvas，避免 AppKit 动画期间残留旧字形。

本地 `TerminalBuffer::new_local` 用 VTE 状态跟踪器处理跨 read 的清屏序列：

- 普通 `CSI 2 J` 原位擦除视口，保留光标、背景和已有历史，不把当前画面推入历史。
- 连续至少 `rows-1` 个换行后紧接 `CSI 1;1 H` 与 `CSI 0 J` 的完整刷新序列，会把仍能放入空白视口的历史回填。这适配 Vite 式刷新，只重排网格存储，不改 PTY 尺寸、不丢保留输出；超出视口的真实内容仍可滚动。
- 普通换行、局部擦除、备用屏和 SSH 保持各自语义；显式 `CSI 3 J` 仍清除历史。

### Shell 报告、标题与光标

`integration.rs` 通过每次连接独立 nonce 的 OSC 报告命令与提示符状态，集成脚本恢复用户 HISTFILE，不扫描用户历史文件。nonce 隔离会话和误识别，不认证恶意远端进程。Bash 已有自定义 DEBUG trap 时不覆盖；无法可靠识别新命令时不收录。PowerShell 保留原 `AddToHistoryHandler` 返回值，不持久化 MemoryOnly/SkipAdding 报告，并保守过滤敏感词；语义见 [PSReadLine 文档](https://learn.microsoft.com/en-us/powershell/module/psreadline/set-psreadlineoption#-addtohistoryhandler)。

`HistoryItem` 仅含 UUID/命令，`HistoryCatalog` 以成员索引保存本地和共享 SSH 两份列表，总上限 5,000。命令首尾 trim 后在所属列表内精确去重，重复执行复用 UUID，只有可靠执行报告才置顶；搜索、复制和填入不改变顺序。格式 2 的一致备份、事务迁移及存储边界见[数据文档](data.md)。`HistoryTarget` 固定弹窗 UUID、完整 Owner 和列表范围；填入/执行还核对活动终端、连接状态与可靠提示符，隐藏父帧或迟到回调不能向其他终端提交。

`command_cursor` 在 ready/running 标记的字节边界捕获提示符末尾，根据真实回显维护可编辑范围；提交输入先使定位失效。单击定位只生成左右方向键，经原 Session 输入通道交给 Shell，等待回显更新。宽字符占位格不算独立字符，换行按 WRAPLINE 处理。无可靠提示符、回滚历史、前台程序及密码输入不走此路径；应用鼠标模式使用 SGR/UTF-8/传统 xterm 协议，拖选和 Shift 选择另行处理。

标题/目录事件只更新原 Owner。本地标签取聚焦窗格的目录或可靠程序元数据，SSH 标签使用资料快照的名称或地址；动态标题不修改会话 ID 和连接资料。

## 布局与弹窗状态

`PaneLayout` 的叶子保存 pane UUID，分支保存独立 UUID、方向、比例和两个子树。布局与终端实体分离，调整几何不重建 PTY、SSH 或编辑器；仅本地标签可分屏，UI 和业务层都限制最多 10 个窗格。损坏或超限的恢复图按有效会话拆成单标签，保留记录。`SavedTab`/`SavedPane` 保存布局、活动窗格、参数快照、工具开闭及最近页面，恢复时创建新的连接尝试。

每个 SSH Pane 持有文件视图、草稿、监控和筛选/滚动状态。文件面板在终端下方，右栏只显示系统概览，编辑器和历史使用弹窗；右栏按窗口客户区与偏好计算，最多 50%，临时限宽不回写偏好。文件首次可见后懒加载，空目录不触发周期性重复请求。

`Workbench::modal` 保存当前弹窗；`modal_stack` 保存普通子弹窗覆盖的 `ModalFrame`，包括弹窗实例、滚动句柄、弹窗焦点和打开前的具体控件焦点。`show_modal` 移动父实例，`dismiss` 恢复最近父帧及可继续操作的焦点，保留输入实体、查询、选区与滚动。确认另用 `modal_confirm_return` 保存业务父帧：删除、主机信任、传输、进程、冲突和未保存关闭等确认不进入普通栈，按固定目标处理取消与成功去向；链路结束清理遗留父帧。

弹窗宽度由 `Modal::preferred_width()` 按内容结构决定，与导航用的 `is_confirmation()` 分开；遮罩 `.inset_0()` 覆盖顶层 `.relative().size_full()` 根节点后居中。`uniform_list` 的 `ListSizingBehavior::Auto` 要求经确定高度的 flex 链得到视口，条目根显式 `.w_full()`；文件树行固定高度与宽度，以 `ListHorizontalSizingBehavior::Unconstrained` 和同一 `UniformListScrollHandle` 管理两轴，目录定位用 `scroll_to_item`，不依赖未绘制行的坐标。尺寸与排版数值统一见[设计规范](design.md)。

连接库的 `connection_rows_cache` 按查询词和全部资料字段指纹缓存过滤快照，滚动帧只构造可见行。键盘与行内动作按当前可见 UUID 重新解析目标，过滤后剔除不可见多选，行按钮阻断冒泡；搜索框只接管无修饰键上下方向键，IME marked range 期间交给输入法，Enter 经单行 Input 的公开 action 路由。

标签栏的独立 `ScrollHandle` 与标题栏几何隔离，标签总宽度不参与祖先最小宽度；活动标签只在新建、切换或尺寸测量后定位，后台输出不拉回用户视角。排序手势固定源 UUID，水平位移达 4px 后按可见标签中点选插入位；栏外释放、Esc 和失焦取消，结束停止边缘滚动。关闭按钮独立命中，排序按 UUID 恢复活动标签。平台标题栏兼容约定见[开发指南](development.md#平台兼容点)。

统一控件层使用 `ButtonTone::{Default, Primary, Success, Warning, Danger}` 与 `theme::ButtonPalette`，外观组合不改变动作语义，禁用覆盖选中，图标跟随状态前景。设置为三组、四个布尔开关，均用 `controls::Switch`；`Workbench` 保留稳定焦点句柄，无修饰键 Space/Enter 首次按下仅切换一次并阻止冒泡，鼠标监听忽略键盘抬起生成的点击，子组件不重复监听。修改经 `toggle_setting` / `apply_preferences` 异步保存。

## 文件、编辑与传输

目录列表由 `files::listing_entry` 从同一 READDIR 回复的属性复制类型、大小、mtime 与权限，缺失字段保持 `None`，不为每行另发 stat；界面经 `files::format_modified` 把 mtime 按本机时区格式化为 `YYYY-MM-DD HH:mm:ss`，未知值显示“—”。`files::sort_entries` 按 `FileSort`（默认，或大小/修改时间的降序、升序）原地排列 `FileTool.entries`：目录始终在前，未知值两向都排最后，同值按名称；排序状态只存在于该面板内存，目录回执到达时按当前排序重排，因此 `visible_paths`、Shift 范围和方向键都使用显示顺序，按路径保存的选区不受重排影响。文件行事件携带 Owner 与目录 request；`file_selection` 只保存当前视图的完整路径、范围起点和键盘终点，不持久化。目录成功更换或刷新后清空选区；行按钮固定本行，批量确认固定路径集合。`ui/file_actions.rs` 用 `FileSnapshot` 与独立操作 UUID 分开处理目录和操作回执。复制来源限制同一 Owner/attempt，可导航到目标目录；重命名与关联读取/保存互斥，成功更新文档路径且只刷新仍匹配的目录。后端复核类型、包含关系和冲突，复制依赖远端 POSIX `cp`。移动与复制共用 `FileClipboard`（`ClipboardMode::{Copy, Move}`，标记即替换），粘贴前由纯函数 `files::plan_move` 词法拒绝空集、无效路径、同目录、移入自身/子目录与批内重名，`files::move_conflicts` 对照当前完整列表（含隐藏项）拒绝冲突；`FileOperation::Move` 再由 `files::move_entries` 在后台全量预检（目标为目录、规范化路径、类型与标记一致、拒绝特殊文件、包含关系、`lstat` 目标不存在）后逐项复检并执行 SFTP RENAME，取消或单项失败只影响剩余/该项，回执 `Event::FileOperation.moved` 返回实际完成的路径对。`relocations` 统一给出重命名与移动的路径映射，用于与读取/保存互斥及成功后更新草稿；每次标记分配独立缓冲 UUID，移动请求保存所属缓冲 UUID，回执仅消费该缓冲，不清除期间重新标记的复制/移动来源；移动回执刷新目录，并以 `ListingFollowUp` 绑定新目录请求 ID，在该列表回执到达时选中移入条目并保留逐项错误。

`Document` 固定原 Owner、读取时的 `DocumentEndpoint`（主机/端口/用户名）、文档 ID、路径、编码/BOM、内容基线、revision 与 preview/lossy。纯函数 `ui::editor::document_binding` 判定文档相对窗格当前 attempt 为 `Current`、`StaleClean` 或 `StaleDraft { can_continue }`：重连清除旧读取/保存/格式化请求并丢弃干净文档，打开入口也丢弃同路径的 `StaleClean` 后重新读取；脏草稿只在同 session、窗格已连接、无保存进行中且端点一致时由用户显式改绑（`continue_document_on_current`），不会自动重绑。输入订阅按文档 ID 查找，改绑后继续跟踪修改。读取只接受最新 request（携带 `OpenIntent`），保存携带内容与版本快照，仅当前版本成功才清除 dirty；预览与 lossy 文档拒绝保存。后端先严格编码并重新读取目标，确认仍为可读普通文件，再写同目录临时文件，以唯一恢复文件保护原内容后按最后保存覆盖；远端内容变化本身不拒绝保存，失败保留草稿和恢复路径。编辑器行列定位也先核对原 Owner/ID，再调用原生 `InputState`，不修改文本。

`editor_format.rs` 在后台一次映射两个 UTF-8 选区端点。`ui/editor.rs` 应用前核对 revision、文本、IME 和视图状态；组件本地补丁以显式编辑事务设置前后撤销边界，恢复正向/反向选区与滚动，使格式化可独立撤销/重做。补丁范围见[开发指南](development.md#依赖与本地补丁)。

传输记录固定连接、Owner、路径和任务 UUID，全应用通过共享 semaphore 限制为最多两个 worker。`Modal::Transfer` 保存批次 UUID、Review/Running/Result 阶段、覆盖选项及原目录/request；提交在注册 worker 前切换 Running，重复、空任务或失效目标不启动。文件选择器回调重新核对会话、连接、路径、request 和下载选区。只有全部成功才自动关闭，并且只刷新仍匹配的原目录；失败或取消保留逐项结果，重试生成新 UUID 的核对页。

关闭进行中的传输视图允许后台继续；停止确认固定当时活跃任务 UUID 并再次复核归属，只请求后端停止，终态由 worker 决定。迟到结果不逆转取消终态。传输记录独立持久化，清理记录前复核 Owner、选区和终态，不删除运行中任务；`transfer_paths` 仅格式化展示，不参与实际路径计算。

## 监控与进程操作

`monitor::SAMPLE_COMMAND` 运行固定 Linux 命令，读取系统、CPU、内存、网络、df、ps 和 ss。CPU/网络按同次开机的两次单调采样计算差值，首次或不可用数据留空并说明；趋势窗口有限且保留缺失分段。连接成功后开始采样，概览可见期间每 3 秒刷新，断开清空。进程、端口和资源弹窗读取所属 Pane 的有效采样，筛选/排序/展开/滚动也属于该 Pane。

进程 `Identity` 由 PID、启动 ticks 和 boot UUID 构成，采样在 ps 前后复核，只为一致实例补身份。`current_process` 要求原身份完全匹配；提交还检查 transport、Owner、request、采样错误及 10 秒新鲜度。详情请求固定实例和最新 UUID，过期回复不能更新当前或确认父帧，读取失败保留上次成功内容。确认固定主机及动作，取消和提交均恢复原详情，发送状态驻留 `process_attempts` 并按 request 接收结果。

后台独立 exec 通道只执行固定脚本，动作枚举仅允许 Terminate(SIGTERM) / Force(SIGKILL)。PID 须大于 2，发送前重新复核 boot UUID、启动 ticks 和内核线程标记；名称、用户、完整命令只作显示，不拼入脚本。详情文本有读取上限并编码。`Outcome::Sent` 只代表发送成功，后续有效采样才确认退出，最长主动复核 10 秒；无法确认则提示仍运行或结果未知，不乐观删行、不自动升级或重试。身份复核与 POSIX kill 之间仍有短暂窗口，不提供 pidfd 级原子保证。

`port_view` 保持 `ss` 原始记录语义，按完整字段筛选、拆分端点、数字排序并用 `PortKey` 区分重复行；无方括号的 IPv6 不猜端口。端口关联的进程详情复用同一身份门禁。Linux 字段与权限语义参考[内核 /proc 文档](https://www.kernel.org/doc/html/latest/filesystems/proc.html)。

## 持久化、关闭与更新

SQLite 访问在后台串行执行，最终工作区保存通过元数据队列 barrier 等待前序操作和本次写入完成，再关闭会话与任务。原数据库无法读取时保留原文件，使用本次运行的内存数据库并明确提示；迁移先一致备份，再事务更新。具体文件、格式与恢复规则集中在[数据文档](data.md)。

自动更新检查固定官方源，选择平台资源，流式下载并校验 SHA-256。下载/准备回执必须匹配本次 request，取消使旧结果失效；手动检查可接管正在进行的自动检查。`CloseTarget::Update` 复用草稿与传输关闭保护，取消清除待更新目标，工作区保存成功后才启动等待应用退出的安装助手。助手启动不代表安装成功；文件、进程和暂存清理都在后台，完整边界见[自动更新](updates.md)。

## 安全边界

- 主机身份确认先于本地凭据读取和发送。`SecretStore` 隔离阻塞操作；正式使用 AES-256-GCM `LocalVault`，无主密码、不调用系统钥匙串。本机随机密钥以 `persist_noclobber` 发布，每次写入使用独立 nonce，已有 key 不被替换。
- 凭据回复不实现 Debug/序列化，秘密用 `Zeroizing`。attempt 固定记忆策略，错误密码不覆盖已保存值；编辑表单不预读，仅在用户点击眼睛时经 `runtime.spawn_blocking` 读取并按请求 ID/资料 UUID 过滤旧结果，判定逻辑在 `credentials::reveal`；原样未改的已显示密码按未输入提交；克隆新 UUID 不复制密码。用户明确确认的 CSV 密码导入是按资料 UUID 写库的独立路径，导出明文密码按敏感文件处理。
- 正式功能连接真实后端，模拟采样仅存在于测试和显式隔离 debug QA。UI 不执行同步网络、文件、数据库或密钥派生，不记录原始键盘输入或凭据；QA 快照只用于隔离测试数据。
- macOS、Windows、远程 Linux 的验证边界见[验收说明](acceptance.md)。Windows x86/x64/ARM64、ConPTY、DPI、输入法和安装体验仍未实机验收；新增 Linux 验证仅使用用户指定的远程环境，禁止创建或运行本地 Linux VM/容器。
