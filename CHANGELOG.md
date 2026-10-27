# 更新日志

源码基线保持 `1.1.10`；下文的源码 tag 用于定位变更，是否已发包以 [GitHub Releases](https://github.com/realmx/MantaSH/releases) 为准。功能摘要保留人工整理，文末的[逐次提交记录](#逐次提交记录自动生成)覆盖本仓库从 `ffe9d05` 开始的完整可达历史，不补造仓库重建以前的提交。自动维护规则见[发布文档](docs/release.md#更新日志自动生成)。

## 当前源码变更

- 修复 SSH 重连后再次打开同一文件仍显示“原连接已失效”、无法编辑和保存的问题：重连时丢弃未修改的旧文档，双击、右键或预览入口重新打开时经当前连接读取最新内容；有未保存修改的旧草稿在编辑器底栏提供“在当前连接继续编辑”，仅当同一会话已连接且主机、端口、用户名与打开时一致才可改绑并通过原保存流程写入（仍为最后保存覆盖），端点变化时仍可编辑或复制草稿，但不能保存到当前连接；旧草稿也可直接放弃，底栏状态改为换行显示不裁切。补充判定纯逻辑测试、QA 动作与验收脚本及文档。
- 编辑/预览弹窗顶部工具栏（文件路径、编码、格式化/编辑按钮）补上与标题栏和底栏一致的内边距：左右 12px（`theme::SPACE_PANEL`）、顶部 8px（`theme::SPACE_CONTROL`），不再贴着弹窗边缘。
- 文件列表名称后各列之间的间距固定为 4px（`theme::SPACE_SMALL`）：大小列到修改时间列、修改时间列到操作列不再随面板宽度在 8px/12px 间切换。
- SFTP 文件列表增加行内预览（Lucide `eye`）与右键“预览/编辑”，动作列改为六个按钮（默认 140px）；放宽文本/日志/配置的打开检测（允许 ANSI ESC 等无害控制字符、扩展名与常见文件名允许列表，UTF-8 失败时尝试 GBK/GB18030，预览可用替换字符回退且不可保存），并区分只读预览与可编辑模式（脏草稿优先打开编辑草稿）。补充允许列表、控制字符、编码回退与预览门禁测试及文档。

- SSH 原标签重连成功后保留窗口滚动历史与重连前画面（视口推入 scrollback 并复用同一 `TerminalBuffer`）；失败或取消仍显示原终端，新建 SSH 标签仍从空网格开始。
- SFTP 文件列表新增移动文件/文件夹：行内“移动”按钮（Lucide `scissors`）或右键菜单标记后，导航到目标目录点击工具栏“移动到此处 (N)”，经 SFTP 重命名完成；工具栏同时显示待粘贴内容与取消按钮，复制也可在此粘贴或取消。拒绝同目录、移入自身或子目录、重名与已有同名目标（不覆盖），后台预检与逐项复检后执行，部分失败逐项报告；成功后刷新、选中移入条目、清除待移动内容并更新草稿路径，重连/关闭失效。行内动作列改为五个按钮（默认 116px），补充纯逻辑与真实 SFTP 测试、QA 场景和文档。
- 本地终端每个标签的分屏上限由 5 个扩大到 10 个，支持左右、上下和混合布局；分屏按钮、操作边界与工作区恢复使用统一上限。补充十窗格创建、超限拒绝、关闭后补回及布局恢复测试，并同步 QA 脚本和使用文档。
- 系统概览网络模块的两项速度由“下载/上传”改为“接收/发送”（英文 Received/Sent），与趋势图接收线、发送线及接口累计量的收发含义一致；SFTP 文件面板的上传、下载文案不变。
- 新建/编辑连接表单的密码框右侧新增眼睛按钮（Lucide `eye`/`eye-off`，提示“显示密码/隐藏密码”），可显示或隐藏输入的密码；每次打开表单默认隐藏、不预读密码，关闭即复位。编辑已有连接且密码框为空、开启密码记忆时，首次点击在后台读取并显示已保存密码（无保存值提示“未保存密码”，读取失败保持隐藏并提示错误），再次点击重新隐藏但保留内容；原样未改的已显示密码保存或连接时不视为新密码、不重写凭据。关闭记忆时不读取。AGENTS.md 安全规则同步增加“用户主动的本机查看与 CSV 导出可不经主机确认读取凭据”的例外。
- SFTP 文件列表的“大小”“修改时间”表头可点击排序：依次切换降序（最大/最新在前）、升序和默认顺序，当前排序列显示箭头；目录始终排在文件前，组内按所选列排序、同值按名称，目录按大小排序时保持名称顺序，未知大小或时间两向都排最后。排序状态保存在当前文件面板内存中，刷新与目录切换时保留、不写入偏好；选区按路径保持，Shift 连选与方向键按新显示顺序计算；表头列宽为排序箭头预留空间，不裁切文字。
- SFTP 文件列表在大小与操作之间新增“修改时间”列（表头同步显示）：直接使用列目录返回的 mtime，按本机时区显示为 `YYYY-MM-DD HH:mm:ss`（含秒），服务器未提供时显示“—”；列宽按界面字体实际字形宽度测量并贴合时间文字、不截断不换行，名称列默认宽度相应让出空间，选择与行内操作不变。
- 更新日志改由本地提交前钩子维护：`.githooks/pre-commit` 在每次提交前按 `HEAD` 历史重写逐次提交记录并随同一提交暂存 `CHANGELOG.md`，不再由 Actions 在每次推送后追加同步提交；日志覆盖到上一提交，最新一条在下次提交时补入。克隆后执行一次 `git config core.hooksPath .githooks` 启用，`MANTASH_SKIP_CHANGELOG=1` 可临时跳过；首次提交、浅克隆和指定路径提交时跳过而不阻止提交，删除 `changelog.yml` 与日志推送逻辑。
- 终端支持 Shift+单击延伸选区：已有选区时保留原起点，把终点连续延伸到点击处（之前、之后或跨行均可，包含两端字符），按住拖动可继续调整并支持边缘自动滚动，双击选词的选区按整词延伸；无选区时 Shift+单击照常开始新选区，Shift 仍绕过鼠标报告且不触发点击定位光标。
- 修复终端自下而上（或从右向左）拖选时选区漏掉终点所在字符的问题：行首第一列的字符（如 `[INFO]` 的 `[`）以及拖选起点字符现在都会被选中和复制，指针移出左边缘或边缘自动滚动时同样生效；正向拖选、双击选词与复制行为不变。
- 终端拖选文本时，指针停在上/下边缘或移出边缘会持续自动滚动回滚历史并延伸选区，速度随越界距离增加，松开、返回终端内或到达历史边界即停止；鼠标报告、滚轮、滚动条与分屏行为不变。
- 清理五个无产品用途或重复的品牌图标，仅保留界面单色 SVG、README PNG、Windows ICO 和 macOS ICNS；生成脚本只输出这四个文件，检查覆盖完整输出清单及 ICNS 各尺寸一致性。
- 品牌统一采用“折峰”四片折叠 M：替换彩色与单色 SVG、文档 PNG、Windows ICO 和 macOS ICNS，更新应用顶栏与关于标识；README 展示新 Logo，并重拍五张黑夜主题原生截图。
- 品牌几何与配色由同一脚本生成，保留 macOS 满画布不透明背景和 Windows 多尺寸图标；同步品牌规范与资源一致性检查。
- 更新 README 的五张 macOS 原生窗口截图，统一使用黑夜主题并保留 2560×1600、2× Retina 分辨率；同步中英文截图说明与验收记录。
- 补齐历史功能摘要，增加完整逐次提交记录；每次推送 `master` 后由 Actions 自动生成并提交日志，包含本次推送内全部提交，保留手写摘要，并避免日志同步循环。

## v1.1.24（源码 tag）

- 统一原生工作台的五种按钮操作色、悬停和禁用状态；SSH 闪电按钮按连接状态着色，连接资料编辑仍集中在连接库。
- 弹窗按内容选择宽度并随字号调整，保留小窗口边距；修复 macOS 应用菜单的关于入口、重复打开及父弹窗恢复。
- 设置改为三组布局，提供跟随系统外观、鼠标报告、连接后打开文件面板、记住认证凭据四个可键盘操作的开关；主题和语言使用蓝色选中态，手动选择主题关闭系统跟随，恢复默认覆盖可见设置。
- 新 SSH 资料预填 `root` / `22`；移除全局端口设置并兼容旧偏好，保留已有资料的自定义用户名与端口。
- 连接库集中搜索、名称排序、导入和添加入口，保留多选、批量删除、克隆及同资料多会话；导出始终覆盖整个连接库。
- 文件列表保留名称、大小、操作三列，提供重命名、复制、下载和删除直接动作；异步操作固定原会话、请求与路径，拒绝旧请求，重命名同步有效草稿。
- 在线编辑增加真实行号与可独立撤销的基础格式化，保留严格编码、保存版本和草稿保护；本地组件补丁提供编辑事务及选区、滚动恢复。
- 历史命令支持行内填入和明确执行，并固定到打开弹窗时的终端；切标签、重连和迟到回调不能把命令送到其它会话。
- 命令历史仅保存 UUID 与命令，本地和 SSH 两份列表共用 5,000 条容量；SQLite 格式 2 先一致备份再事务迁移，元数据有序提交并在退出前等待保存。
- 主机确认先于凭据读取，新密码认证成功后才保存，编辑资料不回填密码；关闭记忆后下次连接不读写已保存密码，但不删除已有凭据。
- CSV 资料事务与密码写入分阶段处理，失败密码可单独重试；导入保留密码首尾空白，空密码不清除旧值。导出遇到凭据读取失败即终止，使用私有临时文件与替换操作，失败时保留原文件。
- 精简进程、端口、传输和设置界面，统一操作间距；补充隔离原生 QA、按钮对比度、数据迁移和异步归属回归。
- 移除浏览器演示与连接示例，归并重复文档，完善功能、数据、平台和验证边界；文档检查增加本地章节锚点校验。

## v1.1.21–v1.1.23（源码 tag）

- 传输记录对已知总量显示进度环，对未知总量显示活动指示；悬停可查看方向、百分比或已传字节。
- 上传/下载的核对、传输中和结果列表统一为状态图标、文件名、大小单行；路径、状态与错误移入悬浮提示，保留核对页行尾移除和失败重试操作。

## v1.1.20（源码 tag）

- SSH 工具栏移除会话编辑入口，连接资料统一从连接库编辑，避免旧会话快照覆盖库内新值。
- 检查更新集中到共享关于弹窗，可从设置版本按钮进入；显示检查进度、新版本、无适用更新或失败结果，支持重试与取消。

## v1.1.15–v1.1.19（源码 tag）

- 调整关于弹窗布局；后续手动检查更新入口在 v1.1.20 集中到关于弹窗。
- SSH 追加旧式 RSA 主机密钥、交换与 MAC 算法兼容配置，保留默认算法优先级；真实旧版服务器仍待专项验收。
- 容量口径统一为十进制 MB：CSV 导入、在线编辑及监控输出的 8 MB 限制统一为 8,000,000 字节，更新下载进度同步换算。
- 更新 SSH 相关依赖以处理已报告的 Dependabot 安全告警，最低 Rust 工具链升至 1.89。
- macOS DMG 校验对临时资源繁忙错误执行有限退避重试；其它错误或重试耗尽仍阻止发包。

## v1.1.11–v1.1.14（源码 tag）

- 修复本地终端清屏与 Vite 刷新造成额外滚动历史的问题，保留真正溢出的终端内容。
- PTY 接受新尺寸后再更新终端网格，缩放失败保留原网格，避免终端与后台尺寸脱节。
- 进程列表显示完整程序及参数，搜索只匹配命令；修复首行对齐，详情平铺完整命令，持续刷新保留已有内容并拒绝旧回复。

## 1.1.10（源码基线；tag v1.1.10）

- 自动检查官方稳定版本，确认后下载、校验并通过平台安装助手更新；安装前保护未保存文件及活动传输。
- macOS 应用图标使用铺满画布的深蓝底色，移除透明外圈。
- 清理无引用的重复 PNG、品牌展示板及其专用字体，保留运行与打包所需资源。
- 源码基线同步为 1.1.10，正式发布版本以 tag 和 Release 页为准；Windows 实机更新安装仍待验收。

## 首版至 1.1.10 的其它源码改进

- 改善 Windows Git Bash 的发现、路径兼容和恢复目录；默认提示符移除顶部多余空行，保留用户自定义提示符。Windows 实机运行仍未验收。
- 关于界面补充作者和 GitHub 信息，并提供 Windows 可见入口。
- 发布流程直接为源码提交创建 tag，构建 runner 临时同步包版本；纯文档更新不增版。改善 tag 推送诊断与受限名称处理，保留服务器拒绝信息，不覆盖已有 tag 或绕过权限。

## 1.1.5（历史源码基线）

- 源码基线同步为 1.1.5，发布版本仍以 tag 和 Release 页为准。
- 导入预览移除统计和提示模块，更新重复连接选项放入 footer，列表增加分隔线、逐行移除按钮和滚动条。

## 1.1.0（历史源码基线）

- 在 1.0.0 的基础上加入 Windows 原生透明标题栏、窗口拖动与系统按钮处理、Windows 应用图标嵌入和无额外控制台窗口启动。
- 增加本地终端 Shell 选择与持久化；Windows 实机验收单独记录，不以构建通过替代。

## 1.0.0

MantaSH 首个公开产品版本，包含：

- 本地真实 PTY、多标签、仅本地终端的五窗格混合分屏、动态标题、命令光标定位和 Shell 集成历史。
- SSH 密码连接、主机指纹验证、连接库 CSV 导入导出、共享命令历史、重连和本机加密凭据记忆。
- SFTP 文件面板、目录树、批量选择、上传下载、传输历史、在线文本编辑和会话编码。
- Linux SSH 服务器的 CPU、内存/Swap、磁盘、网络、进程和端口信息。
- 中英文、白天/黑夜、字体字号、面板尺寸和工作区恢复。
- GitHub Actions 发布 macOS arm64/x64 ad-hoc 签名 DMG、Windows x86/x64/ARM64 Inno Setup 安装器，每个包附 SHA-256 校验文件；不发布便携 ZIP。

版本详情、安装方式、限制和发布流程见[发布文档](docs/release.md)、[功能与范围](docs/features.md)和[用户手册](docs/user-guide.md)。

<!-- BEGIN AUTO-GENERATED COMMIT LOG -->

## 逐次提交记录（自动生成）

本区由提交前钩子按 Git 完整历史自动生成，随每次提交写入，覆盖到该提交的上一提交（最新一条在下次提交时补入）；包含普通提交、合并和文档更新，仅省略旧版 Actions 机器人的日志同步提交。日期为提交日期，正文保留原始说明，不代表对应版本已经发布或通过平台验收。

### 2026-10-26

- **补齐编辑与预览工具栏内边距** · [b36bec6](https://github.com/realmx/MantaSH/commit/b36bec60e347eae75002005c82e7197f46f30010)

<details>
<summary>完整提交说明</summary>

```text
为编辑/预览弹窗的文件路径、编码及格式化/编辑工具栏增加左右 12px 和顶部 8px 内边距，复用 theme::SPACE_PANEL 与 theme::SPACE_CONTROL，使其与弹窗标题栏和底栏对齐。同步设计文档与更新日志功能摘要。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets（189 通过，1 个原生 UI fixture 按默认配置忽略）
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- python3 scripts/check_docs.py
- git diff --check

本次未运行原生窗口视觉验收、Windows 实机或 Linux 远程验证；编译仍有现有未使用项、依赖弃用及未来兼容警告。
```

</details>

- **增加文件只读预览并保留 SSH 重连历史** · [cd17166](https://github.com/realmx/MantaSH/commit/cd171660c93ac2606511cd7742e8a4a436dd76d8)

<details>
<summary>完整提交说明</summary>

```text
文件列表增加行内预览和右键预览/编辑入口，打开意图贯穿符号链接确认与异步读取。只读预览禁用正文修改、保存和格式化；无损预览可切换为编辑，已有未保存草稿优先保留，后端同步拒绝保存只读或有损内容。

扩展日志、配置与常见文本文件名识别，允许 ANSI 控制字符，在 UTF-8 失败时尝试 GBK/GB18030；允许的文本仅在预览路径使用有损回退，继续拒绝 NUL 和明显二进制。文件操作列增加预览按钮，大小、修改时间与操作列之间统一使用 4px 间距。

SSH 原标签重连复用终端缓冲，把原视口保留到滚动历史并处理待连接终端的输出通知；重置旧 ANSI 解析状态、终端模式和光标状态，并在持有终端锁时复核取消，隔离迟到的旧连接输出。补充文本解码、重连历史与真实 SFTP 接口测试，扩展 macOS 文件预览 QA，并同步功能、使用、数据、架构、设计、开发、故障排查及验收文档。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- cargo test --locked --no-default-features --test terminal_updates prepare_reconnect
- cargo test --locked --no-default-features --lib cancelled_feed_waiting_for_grid_cannot_mutate_reconnected_buffer
- python3 scripts/check_docs.py
- Python AST 语法检查：scripts/qa_file_actions_macos.py
- git diff --check

本次未运行原生窗口 QA、指针/键盘驱动验收、Windows 实机或新增 Linux 远程验证。原生 UI fixture 按默认配置忽略；编译仍有现有未使用项、依赖弃用及未来兼容警告。
```

</details>

### 2026-10-24

- **Merge remote-tracking branch 'origin/dependabot/pip/pillow-13.0.0'** · [845664b](https://github.com/realmx/MantaSH/commit/845664b766345f7f137cd01f206bd50db4a553d9)

### 2026-10-23

- **chore(deps): bump pillow from 12.3.0 to 13.0.0** · [833589b](https://github.com/realmx/MantaSH/commit/833589bdc567f58b6db767d8fd7e487bcfc577f1)

<details>
<summary>完整提交说明</summary>

```text
Bumps [pillow](https://github.com/python-pillow/Pillow) from 12.3.0 to 13.0.0.
- [Release notes](https://github.com/python-pillow/Pillow/releases)
- [Changelog](https://github.com/python-pillow/Pillow/blob/main/CHANGES.rst)
- [Commits](https://github.com/python-pillow/Pillow/compare/12.3.0...13.0.0)

---
updated-dependencies:
- dependency-name: pillow
  dependency-version: 13.0.0
  dependency-type: direct:production
...

Signed-off-by: dependabot[bot] <support@github.com>
```

</details>

### 2026-10-24

- **SFTP 文件支持移动与待粘贴操作** · [fa5e280](https://github.com/realmx/MantaSH/commit/fa5e28076f302c1ac192350bf6911711273995d9)

<details>
<summary>完整提交说明</summary>

```text
新增文件与文件夹的行内及右键移动操作，通过 SFTP 重命名批量移动；复制和移动共用会话内缓冲，工具栏显示条目数、粘贴或移动到此处以及取消操作。

在界面和后台校验同目录、移入自身、无效路径、类型变化及重名冲突，保留已完成项并逐项报告失败。按实际移动回执更新草稿路径、刷新目录及选中移入条目；移动回执绑定标记 UUID，避免清除用户后来标记的新缓冲。

补充路径规划、真实 SFTP 移动和缓冲回执竞态测试，扩展 macOS QA 场景，同步中英文 README、功能、使用、数据、架构、设计、开发与验收文档，并添加 Lucide 图标。更新日志生成区由提交前钩子维护。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- cargo test --locked --lib ui::file_actions::tests
- python3 scripts/check_docs.py
- Python AST 语法检查：scripts/qa_file_actions_macos.py
- git diff --check

本次未运行原生窗口 QA、指针/键盘驱动验收、Windows 实机或新增 Linux 远程验证。原生 UI fixture 按默认配置忽略；编译仍有现有未使用项、依赖弃用及未来兼容警告。
```

</details>

### 2026-10-23

- **本地终端分屏上限扩大到十个窗格** · [d60efc9](https://github.com/realmx/MantaSH/commit/d60efc96baac54cc575d30a91c5480f7361ccc8a)

<details>
<summary>完整提交说明</summary>

```text
将每个本地标签的窗格上限从 5 提升至 10；分屏按钮、业务操作和工作区恢复共用同一限制，继续支持左右、上下及混合布局。

扩展边界测试，覆盖十窗格创建、第十一个拒绝、关闭后重新分屏和十窗格布局及活动会话恢复；同步两份原生 QA 脚本、用户与开发文档及更新日志摘要。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets
- cargo check --locked --no-default-features --all-targets
- python3 scripts/check_docs.py
- Python AST 语法检查：scripts/qa_smoke.py、scripts/qa_accept_macos.py
- git diff --check

本次未运行桌面构建、原生 GUI 验收和 Windows 实机检查。原生 UI fixture 按默认配置忽略；编译仍有原有 shell 未使用警告。
```

</details>

### 2026-10-22

- **系统概览网络速度改用接收/发送** · [00600fd](https://github.com/realmx/MantaSH/commit/00600fd1dc010d7c7b260f2ba25f12f0f4182240)

<details>
<summary>完整提交说明</summary>

```text
网络模块的两项速度标签由“下载/上传”改为“接收/发送”（英文 Received/Sent），与趋势图的接收线、发送线以及接口累计量的收发含义保持一致，避免与 SFTP 文件面板的上传、下载混淆。

新增对应的界面文案键，并同步设计文档和更新日志摘要；SFTP 文件面板的上传、下载文案不变。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets（170 通过，1 个原生 UI fixture 按默认配置忽略）
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- python3 scripts/check_docs.py
- git diff --cached --check

本次未进行原生窗口人工验收、Windows 实机或新增 Linux 远程验证。编译仍有已有警告。
```

</details>

- **连接表单支持按需显示已保存密码** · [80458a4](https://github.com/realmx/MantaSH/commit/80458a4a161c2c80fac06de08aeaa15bfe424ee0)

<details>
<summary>完整提交说明</summary>

```text
密码输入框增加眼睛按钮，支持显示和隐藏输入内容，每次打开表单默认隐藏且不预读凭据。编辑已有连接、密码框为空并开启密码记忆时，用户点击后才在后台读取已保存密码；新建、克隆或关闭记忆时只切换输入内容的显示。

按请求 ID、资料 UUID 和当前输入过滤过期读取结果，隐藏时取消待应用的读取。读取失败重新遮蔽并显示通用错误，无保存值显示占位提示。原样未改的已显示密码按未输入提交，继续使用原有凭据，不将其视为新密码写回。

补充读取条件、异步回复归属和提交语义的核心测试，扩展原生 QA 动作及 macOS 凭据验收脚本，同步安全规范与用户、设计、数据、架构、功能和验收文档。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets（170 通过，1 个原生 UI fixture 按默认配置忽略）
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- python3 scripts/check_docs.py
- python3 -m py_compile scripts/qa_credentials_macos.py
- git diff --cached --check

本次未运行 macOS 原生凭据验收脚本、窗口人工验收、Windows 实机或新增 Linux 远程验证。编译仍有已有警告。
```

</details>

- **SFTP 文件列表支持按大小和修改时间排序** · [bf2c0af](https://github.com/realmx/MantaSH/commit/bf2c0af508e8dae40f589c3887391beb99209b19)

<details>
<summary>完整提交说明</summary>

```text
大小和修改时间表头改为可点击按钮，按降序、升序、默认顺序循环，显示排序方向箭头和中英文提示。按实际字宽为表头文字、箭头和按钮间距预留空间。

统一文件排序规则：目录始终在文件前，未知值在各组内始终靠后，同值按名称确定顺序；按大小排序时目录保持名称顺序。排序状态仅存于当前面板，刷新和目录切换后继续应用，路径选区保持，Shift 连选及方向键使用新的显示顺序。

补充排序循环、目录分组、缺失值、同值与大小写名称、排序后选区和表头列宽的回归测试，同步架构、数据、设计、开发、功能及用户文档。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets（167 通过，1 个原生 UI fixture 按默认配置忽略）
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- python3 scripts/check_docs.py
- git diff --cached --check

本次未进行原生窗口人工验收、Windows 实机或新增 Linux 远程验证。编译仍有已有警告。
```

</details>

### 2026-10-21

- **按实际字宽收紧 SFTP 修改时间列** · [6174500](https://github.com/realmx/MantaSH/commit/61745005dc33251b354d5e09e3f1e2a604014097)

<details>
<summary>完整提交说明</summary>

```text
修改时间列改用当前界面字体的字形 advance 计算宽度，每个数字位取最宽数字，与表头宽度比较后增加统一的 4px 余量，减少旧估算方式造成的多余留白。

在布局模块提取时间模板和列宽计算函数，对不可用或无效的字形测量提供分类字宽回退；补充比例数字、宽表头、缺失字形及 NaN 测量的回归测试，并同步设计、开发文档和更新日志摘要。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- python3 scripts/check_docs.py
- git diff --cached --check

原生 UI fixture 按默认配置忽略；本次未进行原生窗口人工验收、Windows 实机或新增 Linux 远程验证。编译仍有已有警告。
```

</details>

- **SFTP 文件列表显示修改时间** · [dcffe9f](https://github.com/realmx/MantaSH/commit/dcffe9fd2fcf58a3241ca007abb97563f598a20c)

<details>
<summary>完整提交说明</summary>

```text
在大小与操作之间增加修改时间列，按本机时区显示 YYYY-MM-DD HH:mm:ss，服务器未提供 mtime 时显示占位符。表头与数据行保持列宽一致，按时间样式和字号分配宽度，并调整名称列默认可用空间。

提取 READDIR 属性到列表项的转换，保留服务器返回的修改时间、大小和权限，不额外逐行查询。补充时区、秒精度、时间边界、缺失属性及真实 SFTP 列表传递 mtime 的测试，同步功能、用户、设计、数据和开发文档。

验证通过：
- cargo fmt --all -- --check
- cargo test --locked --no-default-features --all-targets
- cargo check --locked --no-default-features --all-targets
- cargo check --locked --all-targets
- python3 scripts/check_docs.py
- git diff --cached --check

原生 UI fixture 按默认配置忽略；未进行原生窗口人工验收、Windows 实机或新增 Linux 远程验证。编译保留已有警告。
```

</details>

### 2026-10-20

- **忽略 Syncthing 的 .stversions 版本目录** · [976a586](https://github.com/realmx/MantaSH/commit/976a58690c40f137b9cda04da24a0e98c336b197)

<details>
<summary>完整提交说明</summary>

```text
启用 Syncthing 文件版本后，同步文件夹下会生成 .stversions 目录存放历史副本；它属于
本地同步状态，不应进入版本库，因此在 .gitignore 中忽略该目录。
其余忽略规则不变。

验证：git check-ignore -v .stversions 命中 .gitignore 第 22 行；该目录当前尚不存在，
不涉及已跟踪文件。本次未改代码，未运行构建或单元测试。
```

</details>

- **更新日志改由提交前钩子维护** · [6084fe8](https://github.com/realmx/MantaSH/commit/6084fe8069cf3f84c7bbeb06ad45a945e6855012)

<details>
<summary>完整提交说明</summary>

```text
用仓库内 .githooks/pre-commit 替代 .github/workflows/changelog.yml：每次提交前按 HEAD
的完整可达历史重写 CHANGELOG.md 的逐次提交记录，并只把该文件暂存进同一提交，不再由
Actions 在 push 后追加日志同步提交。删除 changelog.yml 和 publish 流程，新增
scripts/install_hooks.sh 与跳过开关 MANTASH_SKIP_CHANGELOG。

hook 覆盖 --amend（改为记录被修订提交的父提交）、首次提交、浅克隆、git commit <路径>
的临时索引、CHANGELOG.md 未暂存修改和生成区内手改等边界；generate 改为按 HEAD 重写
工作区日志，check 只验证工作区日志是否覆盖到 HEAD。同步更新 AGENTS.md、CONTRIBUTING.md、
docs/development.md、docs/release.md、CHANGELOG.md 与 tests/test_changelog.py。

验证：python3 -m unittest discover -s tests -p 'test_changelog.py'（16 项通过）；
python3 -m unittest discover -s tests -p 'test_check_docs.py'（5 项通过）；
python3 scripts/check_docs.py；python3 scripts/update_changelog.py check。
未运行 Rust 检查，本次只改 Python 脚本、钩子与文档。
```

</details>

- **支持终端 Shift+单击延伸选区** · [deab9c5](https://github.com/realmx/MantaSH/commit/deab9c59afa87c6ddd03d6b7963f69c2b49251ed)

<details>
<summary>完整提交说明</summary>

```text
保留已有选区的原始锚点和选区类型，Shift+单击更新终点并包含两端字符，后续拖动沿用边缘自动滚动；无有效选区时回退到新建选区。补充正反向、跨行、滚动、整词和空选区测试，并同步用户指南、架构说明和功能摘要。
```

</details>

- **Merge pull request #4 from realmx/dependabot/cargo/russh-0.64.1** · [263de44](https://github.com/realmx/MantaSH/commit/263de44a1f71c392456a0c32af1f30094c295e0e)

<details>
<summary>完整提交说明</summary>

```text
chore(deps): bump russh from 0.63.2 to 0.64.1
```

</details>

- **chore(deps): bump russh from 0.63.2 to 0.64.1** · [744d33a](https://github.com/realmx/MantaSH/commit/744d33ac9299774461e94a4305b5c93a41245f3e)

<details>
<summary>完整提交说明</summary>

```text
Bumps [russh](https://github.com/warp-tech/russh) from 0.63.2 to 0.64.1.
- [Release notes](https://github.com/warp-tech/russh/releases)
- [Commits](https://github.com/warp-tech/russh/compare/v0.63.2...v0.64.1)

---
updated-dependencies:
- dependency-name: russh
  dependency-version: 0.64.1
  dependency-type: direct:production
...

Signed-off-by: dependabot[bot] <support@github.com>
```

</details>

- **修复终端反向拖选并支持边缘自动滚动** · [087810a](https://github.com/realmx/MantaSH/commit/087810a66b8c0b70ea5b2fdd8e20748271f82440)

<details>
<summary>完整提交说明</summary>

```text
按拖选方向包含起点和终点字符，修复从下往上或从右往左选择时漏掉行首和锚点字符的问题。拖选期间在窗口级跟踪指针，停在终端上下边缘或移出边缘时每 40ms 滚动历史并延伸选区，按越界距离分级提速；松开、返回内部或到达历史边界后停止。

补充自动滚动速度、历史上下界、反向选择及首列复制的回归测试，同步用户手册、架构说明和手写更新摘要；将 Syncthing 的 .stignore、.stfolder 加入忽略规则。

验证：cargo fmt --all -- --check；cargo test --locked --no-default-features --all-targets（157 通过、0 失败、1 个专用 fixture ignored）；cargo check --locked --no-default-features --all-targets；python3 scripts/check_docs.py（22 份必需文档、241 个本地链接与锚点）；cargo check --locked --all-targets；cargo build --locked；git diff --check。macOS arm64 下 cargo run --locked 已启动并确认原生窗口可见。

未执行拖选指针驱动或人工交互验收、Windows 实机及安装包验收；构建仍有既有弃用接口和未使用代码警告。
```

</details>

### 2026-10-17

- **清理冗余品牌图标并精简资源生成** · [a261246](https://github.com/realmx/MantaSH/commit/a261246fef9b836aba326d70a6ba53cc28bbc302)

<details>
<summary>完整提交说明</summary>

```text
删除未被运行时或打包使用的带底彩色 SVG、透明彩色 SVG、备用黑白 SVG 和 macOS 1024px 预览 PNG，仅保留实际使用的界面单色 SVG、README PNG、Windows ICO 和 macOS ICNS。界面全部 20 个操作图标和命令执行图标仍有调用，保持原样。

精简品牌生成脚本，只输出四个正式文件；macOS 单平台模式只生成 ICNS。同步品牌文档和手写更新摘要，资源检查验证完整输出集合、单平台生成隔离及 ICNS 各尺寸与已提交资源一致。

实际重新生成后的保留图标与五张 README 截图逐字节一致，其余 398 个已跟踪文件未变。清理本轮日志、临时目录和 Pillow 工具环境。

验证：cargo fmt --all -- --check；cargo test --locked --no-default-features --all-targets（154 通过、1 ignored）；核心及桌面 cargo check --all-targets；cargo build --locked；Pillow 12.3.0 图标测试 3 通过；文档检查 22 份必需文件及 241 个本地链接/锚点；Python 语法、自动日志状态及 git diff 检查通过。未执行 Windows 实机、安装包或新截图验收，运行时图标与既有截图内容未改变。
```

</details>

- **统一采用折峰 Logo 并更新全部原生截图** · [4e633b8](https://github.com/realmx/MantaSH/commit/4e633b8d1cfb5d22321f735bfdf099ec49a495a2)

<details>
<summary>完整提交说明</summary>

```text
将选定的 04「折峰」四片折叠 M 统一用于彩色、透明、单色、黑色和白色 SVG，以及文档 PNG、Windows ICO 和 macOS ICNS。几何与配色集中在生成脚本，保持选稿比例、主题着色和 macOS 不透明满画布规则，扩展已提交资源的一致性检查。

使用包含新品牌资源的原生 debug 构建，重拍本地分屏、SSH 文件工作区、在线编辑、上传核对和设置五张黑夜截图，保留 1280×800 窗口与 2560×1600 Retina 原图。核验真实回环 SSH/SFTP、编辑器内容和关于窗口标识，同步中英文 README、品牌规范、开发文档、验收记录和功能更新摘要。

清理选稿目录、绘图脚本、预览服务、截图中间文件、隔离应用/SSH 数据及临时工具环境，保留正式资源和可复现生成脚本。

验证：cargo fmt；核心测试 154 通过、1 ignored；核心/桌面 cargo check 与 debug build；Pillow 12.3.0 下图标测试 3 通过；发布脚本测试 35 通过；22 份必需文档及 244 个本地链接/锚点；自动日志状态、Python 语法与 git diff 检查。Windows 实机、安装后的系统图标缓存与安装包签名验收未执行。
```

</details>

- **修复自动日志 workflow 的 runner 上下文位置** · [dc1755a](https://github.com/realmx/MantaSH/commit/dc1755adf5b325f9a9efe62067c6b3e93bb601fc)

<details>
<summary>完整提交说明</summary>

```text
将 TMPDIR 的 runner.temp 表达式从 job 级 env 移到执行步骤的 env，使 GitHub 能解析并运行更新日志 workflow；补齐此前源码提交的生成记录。

验证：actionlint 对 changelog.yml 与 ci.yml 的检查通过；日志生成与完整性检查、文档检查和 git diff --check 通过。生成器实现未变，沿用已通过的 14 项专项测试。
```

</details>

- **补齐完整更新日志并自动同步每次 master 提交** · [0f6e6c2](https://github.com/realmx/MantaSH/commit/0f6e6c22c32dc074fd056bb7983bc2dd9e63c9c7)

<details>
<summary>完整提交说明</summary>

```text
按现有源码 tag 补齐终端清屏与缩放、进程命令搜索、容量限制、SSH 安全依赖、传输进度和原生功能摘要，纠正旧条目的版本归属；保留历史发布与平台验收边界，并生成现有 40 条完整提交记录。

新增更新日志生成器及 master push/workflow_dispatch 自动化，记录全部可达提交的标题、正文、日期和链接，保留手写摘要。通过隔离检出和非强制推送同步，处理并发提交、重复运行和服务端拒绝；只排除专用尾注标识的单亲日志同步提交，不递归触发 workflow 或创建发布版本。

同步贡献、开发、发布和 AI 维护规范，将针对性测试接入 CI。提交正文中的合法空白、代码围栏和冲突标记按原文保存，不阻止日志同步。

验证：14 项真实临时 Git 仓库专项、154 项核心测试（1 个显式 fixture ignored）、5 项文档测试和 35 项发布脚本测试通过；格式、核心编译、Python 与 YAML 语法、22 个必需文档及 240 个链接/锚点、完整历史覆盖与重复生成检查通过。未新增原生 UI、Windows 实机或安装包验收。
```

</details>

- **补充 README 黑夜截图更新日志** · [f23b516](https://github.com/realmx/MantaSH/commit/f23b5166bf18636c62ed3fa7f2cef28e509b8f04)

<details>
<summary>完整提交说明</summary>

```text
在当前源码变更中记录五张 macOS 原生截图统一使用黑夜主题、保留 2× Retina 分辨率，以及中英文截图说明和验收记录的同步更新。

验证：python3 scripts/check_docs.py 与 git diff --check 通过。
```

</details>

- **更新 README 原生截图并统一黑夜主题** · [e1adfce](https://github.com/realmx/MantaSH/commit/e1adfce699d179b83f0d19dd7fc6e0bf1ffcda3a)

<details>
<summary>完整提交说明</summary>

```text
更新本地分屏、SSH 文件工作区、在线编辑、上传核对和设置五张 macOS 原生窗口截图；SSH 工作区与编辑器重新使用黑夜主题拍摄，整组保留 2560×1600 Retina 原始像素。

同步中英文 README 的截图说明，并记录源码构建、隔离回环 SSH/SFTP 场景和原生截图证据范围。

验证：逐张核对黑夜主题、PNG 完整性、尺寸和捕获哈希；文档检查通过 22 个必需文件及 233 个链接与锚点；git diff --cached --check 通过。
```

</details>

- **完善原生功能与数据持久化，清理示例并重整文档** · [8cf26ec](https://github.com/realmx/MantaSH/commit/8cf26ecf16bd859dfa6915ade680a27dd71a156a)

<details>
<summary>完整提交说明</summary>

```text
统一五种按钮操作色、设置开关与选中态、内容自适应弹窗和 SSH 状态图标，修复 macOS 关于入口；新增资料使用 root/22，并兼容已移除的全局端口偏好。

完善文件行操作、在线编辑格式化及撤销、固定历史提交目标、元数据有序保存和 SQLite 格式 2 一致备份迁移，保留认证成功后保存凭据及 CSV 分阶段写入；随附 gpui-component 本地补丁、许可证和针对性回归。

移除连接示例及本地浏览器演示，归并重复文档，按使用、工程维护和发布验收重新组织完整功能说明。修正文档中的排序、并发传输、CSV 导出、编码和平台验证边界；文档检查器增加章节锚点校验及 CI 测试。

验证：Rust 格式、154 项核心测试（1 个显式启动的 fixture ignored）、核心/桌面检查与构建、3 项控件测试、6 项更新测试、5 项文档测试、35 项发布脚本测试通过；22 个必需文件、230 个本地链接/锚点及 42 个 Python 文件语法检查通过。Windows 实机与新的安装包验收未执行。
```

</details>

### 2026-10-14

- **Merge pull request #2 from realmx/cursor/compact-transfer-rows-cb66** · [8e64423](https://github.com/realmx/MantaSH/commit/8e6442334b46fbbb7a8a340614ee6db78779872a)

<details>
<summary>完整提交说明</summary>

```text
fix(ui): compact transfer review rows
```

</details>

- **fix(ui): compact transfer review rows** · [431956c](https://github.com/realmx/MantaSH/commit/431956c00ed4ad5865cdb8203e08dc0d533de56a)

<details>
<summary>完整提交说明</summary>

```text
The upload/download confirm page now uses the same single row as the
running and result phases: direction icon, ellipsized file name and size,
with the per-item remove button at the end of the row. Source and target
paths move into the hover tooltip, which now sits on the icon/name/size
area so it does not overlap the remove and retry buttons' own tooltips.
The overwrite checkbox and cancel/start buttons are unchanged.

The macOS modal QA script now measures the name cell instead of the
source path column, expects one-line rows (< 48px), and requires long
names to leave the list height unchanged instead of wrapping.

Update the design doc, user guide and changelog accordingly.

Co-authored-by: Realm <realmx@users.noreply.github.com>
```

</details>

- **Merge pull request #1 from realmx/cursor/compact-transfer-rows-cb66** · [fc0920b](https://github.com/realmx/MantaSH/commit/fc0920bb4559e1cfa9be11efea4b2d84dbc41141)

<details>
<summary>完整提交说明</summary>

```text
fix(ui): compact transfer dialog rows after submission
```

</details>

### 2026-10-13

- **fix(ui): compact transfer dialog rows after submission** · [95f8e65](https://github.com/realmx/MantaSH/commit/95f8e65ae7e905aaf94d7c5b3d685810d1acb278)

<details>
<summary>完整提交说明</summary>

```text
In the running and result phases of the transfer dialog, each item is now a
single row: the 24px status slot (hourglass, determinate ring, spinner for
unknown totals, green check, red alert/ban), the file name truncated with an
ellipsis, and the size. Source/target paths, the status text and the error
line move into a row hover tooltip that wraps at 360px; failed items keep an
inline retry button. The review phase keeps its full source/target layout.

The name is painted from a canvas shaped against its final bounds because
GPUI 0.2.2 text truncation narrows style runs during intermediate flex
measurement passes, which left long names clipped or only partly semibold.

Update the design doc, user guide and changelog for the compact rows.

Co-authored-by: Realm <realmx@users.noreply.github.com>
```

</details>

### 2026-10-10

- **fix(ui): show ring progress in transfer records** · [0f9fb14](https://github.com/realmx/MantaSH/commit/0f9fb14993c633a7ecfd9588df0438d1ba868fb3)

<details>
<summary>完整提交说明</summary>

```text
Reuse the chart arc for running uploads and downloads in transfer history. Show a spinner for unknown totals, retain direction and progress details in tooltips, and document the behavior.
```

</details>

### 2026-10-08

- **feat: 移除 SSH 工具栏的会话编辑入口** · [a809f45](https://github.com/realmx/MantaSH/commit/a809f454b474ebed1e108727c5d7e5e8ec4fa9f6)

<details>
<summary>完整提交说明</summary>

```text
连接资料编辑统一在连接库完成，会话工具栏不再提供编辑按钮，同时消除用会话快照回填表单可能覆盖库内新值的路径。

连接库行内编辑、克隆、添加与重连入口保持不变。

验证：Rust 1.89 格式与桌面编译通过，126 项核心测试通过（9 项 PTY 用例为沙箱环境限制，与基线一致），文档检查通过。
```

</details>

- **feat: 检查更新移入关于弹窗** · [a6ff946](https://github.com/realmx/MantaSH/commit/a6ff9462c8ecd19432d550af28b772b2c44e0a10)

<details>
<summary>完整提交说明</summary>

```text
关于弹窗 footer 提供“检查更新”，与版本信息同处；设置 footer 仅保留靠右的“恢复默认”。

更新弹窗关闭后返回关于弹窗，父级捕获由设置改为关于。

新增 QA about 动作并更新 qa_updates_macos.py 入口，同步用户、更新与开发文档。

验证：原生更新 QA 全部通过（含关闭返回关于）；Rust 1.89 格式、桌面编译、126 项核心测试与文档检查通过。
```

</details>

### 2026-10-07

- **fix: 重试 macOS 镜像校验的临时资源错误** · [18a0251](https://github.com/realmx/MantaSH/commit/18a02518e46484b9375d93334c57c25e270e8d40)

<details>
<summary>完整提交说明</summary>

```text
将 DMG 完整性验证接入独立脚本，仅对资源暂时不可用或繁忙按 2、4、8、16 秒退避，最多执行五次；永久错误和重试耗尽仍阻止发布。

保留校验和、只读挂载、架构、版本及 ad-hoc 签名检查，补充六项回归测试和发布重跑说明。

验证：35 项发布脚本测试、Python 语法、Rust 1.89 格式与核心编译检查、文档检查通过。本机原生 DMG fixture 创建失败，实际安装包验证交由发布 runner。
```

</details>

- **fix: 修复 SSH 依赖的 Dependabot 安全告警** · [2118a14](https://github.com/realmx/MantaSH/commit/2118a147d9f415e9feba97b7373372c883d7fd66)

<details>
<summary>完整提交说明</summary>

```text
将 russh 升级至 0.63.2、pageant 升级至 0.2.3，并同步锁文件和依赖许可证清单。

将最低 Rust 版本及 CI、发布构建提升至 1.89，同步开发文档。

验证：135 项核心测试及 29 项发布脚本测试通过，Rust 1.89 格式、核心与 macOS 桌面编译检查通过。
```

</details>

### 2026-09-30

- **fix: 统一十进制容量单位** · [a0fbb0d](https://github.com/realmx/MantaSH/commit/a0fbb0d9faafe73a56e02fe26f68f9f07db73494)

- **fix: 兼容 CentOS 6 SSH 算法** · [6a6d625](https://github.com/realmx/MantaSH/commit/6a6d625a8a5c12a085951088802536004f3c67de)

- **fix: 移除关于 MantaSH 弹窗底部操作区** · [6f0d14b](https://github.com/realmx/MantaSH/commit/6f0d14b1d2352da75e534ac8faa55d3d57dbec3f)

<details>
<summary>完整提交说明</summary>

```text
隐藏关于弹窗的 footer 容器、分隔线和占位间距，保留右上角关闭按钮与 Esc 关闭入口；同步用户指南和开发文档。
```

</details>

- **fix: 统一本地 PTY 缩放并补齐终端边界验证** · [f332deb](https://github.com/realmx/MantaSH/commit/f332deb4fcf7de9439fb1c4170a74848ae6dc71d)

<details>
<summary>完整提交说明</summary>

```text
- Unix 与 Windows 路径在 PTY 接受尺寸后才提交本地网格变更，失败保留原网格。
- Windows 缩放失败时退避重试，并优先处理输入、关闭与更新的尺寸。
- 增加提示符重排、真实滚动历史及视口边界回归测试。
- 添加可指定 Bash/Zsh 的原生 PTY QA 脚本和相关 Windows、开发文档。
- 一并提交工作区剩余的进程分组中英文文案。
```

</details>

- **fix: 修复进程列表对齐并优化详情布局与刷新** · [bc6449d](https://github.com/realmx/MantaSH/commit/bc6449d22e53e9953650ee9e3e4fcfa89e95f1cf)

<details>
<summary>完整提交说明</summary>

```text
- 将布局测量元素移出正常排版，统一首行 PID 和命令列对齐。
- 详情摘要改为紧凑双列，完整命令和补充状态平铺显示并统一滚动。
- 移除重复状态字段及刷新加载提示，保留旧内容并仅接受最新有效回复。
- 保持详情打开期间按所属会话持续采样，避免重叠请求和过期数据覆盖。
- 更新中英文文案、使用文档及首行对齐、字号间距、身份校验和刷新回归检查。
```

</details>

### 2026-09-29

- **fix: 修复 Vite 刷新滚动并统一进程命令展示与筛选** · [aac848b](https://github.com/realmx/MantaSH/commit/aac848bbf81216df342b6a650f141c8e2c92e277)

<details>
<summary>完整提交说明</summary>

```text
识别本地终端连续换行填充、回顶和擦除下方的刷新序列，回填可容纳的历史内容，保留实际输出及溢出滚动。增加网格和滑块 QA 诊断、Vite 8.3.0 原生验证脚本与刷新回归测试。

进程列表将名称列改为命令，使用 ps -ww args 采集程序及参数，长命令支持悬停查看；筛选只匹配同一命令字段，不再额外匹配 PID 或用户名。补充关键词匹配、无结果和中英文标题检查，并同步文档。

验证：cargo fmt --all -- --check；cargo test --locked --no-default-features --all-targets（131 通过、1 忽略）；cargo check --locked --no-default-features --all-targets；cargo check --locked --all-targets；cargo build --locked；check_docs.py 和差异检查通过。真实 Vite 原生 QA 8 项、隔离进程列表 QA 10 项通过。Windows 实机与真实 Linux 进程采集未验证。
```

</details>

- **fix: 修复本地终端清屏产生多余滚动条并调整弹窗 footer** · [f2da779](https://github.com/realmx/MantaSH/commit/f2da77931a0f5c546c010ebd527a0a30d8ea734f)

<details>
<summary>完整提交说明</summary>

```text
本地会话的 CSI 2 J 改为原位擦除视口，避免将未溢出的画面转入滚动历史；保留光标、背景、真实历史和 SSH 既有行为。新增分段清屏、溢出滚动、光标背景及备用屏幕回归。

设置 footer 左侧放恢复默认、右侧放检查更新，移除设置版本号和关于弹窗的取消按钮；更新检查逻辑保持不变，同步用户与开发文档。

验证：cargo fmt --all -- --check；cargo test --locked --no-default-features --all-targets（127 通过、1 忽略）；cargo check --locked --no-default-features --all-targets；cargo check --locked --all-targets；cargo build --locked；python3 scripts/check_docs.py；git diff --check 均通过。macOS 原生窗口真实 npm/PTY 复现确认 41 行尺寸一致，清屏短输出历史由 10 行变为 0，真实溢出可回滚并返回末尾。Windows ConPTY 实机未验证。
```

</details>

- **feat: 实现应用更新并将版本基线升至 1.1.10** · [aa3285b](https://github.com/realmx/MantaSH/commit/aa3285b49b77441c8670a2e26c7ddb1b85e38766)

<details>
<summary>完整提交说明</summary>

```text
接入官方稳定版本自动检测、确认下载、SHA-256 校验及 macOS/Windows 安装助手；设置增加手动检查入口，显示进度与结果，支持重试、取消和过期请求过滤，安装前保护未保存文件与活动传输。

调整 macOS 图标为不透明满画布底色；清理无引用 PNG、品牌展示文件及其专用字体，同步图标生成、发布资源、许可清单和文档。将 Cargo、锁文件与基线说明同步为 1.1.10，补充更新、图标及版本发布回归测试。

验证：cargo fmt --all -- --check；cargo test --locked --no-default-features --all-targets（123 通过、1 忽略）；cargo test --locked --lib ui::updates::tests（6 通过）；两种特性的 cargo check --locked --all-targets；Python unittest（32 通过）；check_docs.py 和差异检查通过。此前原生 QA 验证更新提示与取消；Windows 实机安装和正式应用覆盖重启未执行。
```

</details>

- **fix(windows): 统一 Git Bash 路径并消除提示符顶部空行** · [eee37ab](https://github.com/realmx/MantaSH/commit/eee37ab082afe7cc5c575d785c3afc2fd1b4f17a)

<details>
<summary>完整提交说明</summary>

```text
按目录段拼接 Git Bash 路径，并以路径语义匹配设置中的当前 Shell，兼容已保存的混合分隔符路径。仅在 Windows 本地 Bash 初始化中移除默认 Git 提示符的起始换行，保留自定义提示符及历史、目录钩子。补充提示符回归测试和 Windows 文档。

验证：cargo fmt --all -- --check；cargo test --locked --no-default-features --all-targets；cargo check --locked --no-default-features --all-targets；cargo check --locked --all-targets；python3 scripts/check_docs.py；git diff --check 均通过。Windows 构建及实机验收尚未执行。
```

</details>

- **fix(windows): 修复 Git Bash 会话恢复路径** · [ffc4db8](https://github.com/realmx/MantaSH/commit/ffc4db88528a47cfcb7c6f740c192a9fe36e478f)

- **test: 稳定 PTY 输出唤醒测试** · [fc63053](https://github.com/realmx/MantaSH/commit/fc63053ba61b36e2b0c6d2b085c1affb3dcf068b)

- **feat(windows): 添加可见 About 入口** · [79cb566](https://github.com/realmx/MantaSH/commit/79cb5663ca97fc37352c85c2b85a57d880637c7f)

- **chore: 更新版本基线至 1.1.5** · [b9c5bab](https://github.com/realmx/MantaSH/commit/b9c5babc4ab3512cfa9a70065b3af34b578d9904)

- **feat(about): 添加作者与 GitHub 信息** · [b40ec4a](https://github.com/realmx/MantaSH/commit/b40ec4aa2ce604dc75bf9e4db6928525335511a1)

- **fix(windows): 修正原生窗口句柄获取** · [b75d3a4](https://github.com/realmx/MantaSH/commit/b75d3a4b02c11fda3a720c73deef36dc61b57c8f)

- **fix(windows): 修复自绘标题栏拖动与窗口按钮** · [1ed22a9](https://github.com/realmx/MantaSH/commit/1ed22a997aee20739ffdec34d13009c5a141c259)

- **fix(windows): 支持 Git Bash 并保留自绘标题栏** · [1249c57](https://github.com/realmx/MantaSH/commit/1249c574c6808fd0141c1416f864c3bdd7524d15)

- **feat: 升级 1.1.0 并完善 Windows 窗口与 Shell 设置** · [a2e09af](https://github.com/realmx/MantaSH/commit/a2e09af386375d350b7210dbb5879568cb868a8e)

<details>
<summary>完整提交说明</summary>

```text
将源码基线升级为 1.1.0，保留历史 1.0.0 发布记录。完善 Windows 透明标题栏、原生窗口按钮、顶部空白拖动、应用图标和 GUI 子系统；新增本地 Shell 异步发现、下拉选择、持久化及不可用 Shell 显式报错。

更新设置弹窗布局、文案、用户/开发/发布文档和设置截图。验证：cargo fmt、默认及无桌面 cargo check、核心测试、29 项发布测试、文档检查和 git diff --check 均通过。Windows 实机交互尚未验收。
```

</details>

### 2026-09-28

- **docs: 统一界面预览截图与圆角透明背景** · [0464d60](https://github.com/realmx/MantaSH/commit/0464d6020d2bb61d5c8508bb432536d91f4f0ff4)

<details>
<summary>完整提交说明</summary>

```text
使用当前 MantaSH 窗口重新生成五张界面预览图，统一为 2560x1600，并保留窗口原生圆角与透明 alpha；设置截图同步最新 footer 版本布局。

验证：python3 scripts/check_docs.py、git diff --check、五张 PNG 尺寸与 alpha 边界检查通过。
```

</details>

- **docs: 更新界面预览截图与 Windows 截图验收说明** · [8a6b0b5](https://github.com/realmx/MantaSH/commit/8a6b0b5e356d94dd48f4c180177ff15a5b812da5)

<details>
<summary>完整提交说明</summary>

```text
使用当前打开的 MantaSH 窗口重新截取本地终端、SSH 工作区、在线编辑、传输核对和设置版本五张预览图，统一为 2560x1600。补充 Windows 截图必须来自实机的验收边界。

验证：python3 scripts/check_docs.py、git diff --check 通过。截图来自用户当前运行窗口；未额外启动应用。
```

</details>

- **fix(release): 跳过受限版本标签并继续自动发布** · [133e44d](https://github.com/realmx/MantaSH/commit/133e44d78cf714a3b48a05882bda3356c727fb6b)

<details>
<summary>完整提交说明</summary>

```text
当 GitHub 明确拒绝某个 tag 名称并返回 creations are restricted 时，自动递增补丁版本并重试；其它权限、网络和规则错误仍直接失败。修复完整 tag 列表下的重写历史版本计算。

补充 v1.0.1 拒绝后创建 v1.0.2 的回归测试，更新发布文档。29 项发布测试、Python 语法、文档检查和差异检查通过。
```

</details>

- **fix(windows): 将应用图标嵌入桌面可执行文件** · [887f52c](https://github.com/realmx/MantaSH/commit/887f52c82ea5120bae73fe1570fa4849fa71cfec)

<details>
<summary>完整提交说明</summary>

```text
通过 build.rs 和现有 embed-resource 将 mantash.ico 以资源 ID 1 链接到 mantash.exe，供 GPUI、任务栏、程序文件、快捷方式和卸载项读取。保留 GPUI 原有 manifest。

Windows 打包前以资源映射方式核验 EXE 图标，缺失则失败；补充缺图标拒绝及句柄释放测试，更新 Windows 文档。格式、桌面与核心编译、核心测试、28 项发布测试和文档检查通过。
```

</details>

- **fix(windows): 启动桌面程序时不创建额外控制台窗口** · [d1c2fe3](https://github.com/realmx/MantaSH/commit/d1c2fe3f267c46026a5e20389fd4869d06f92d0b)

<details>
<summary>完整提交说明</summary>

```text
Windows 桌面入口统一使用 GUI 子系统，应用内本地 Shell 保持 ConPTY 路径。安装器打包前校验 PE32/PE32+ 的 GUI 子系统，拒绝控制台程序和损坏的头部。补充回归测试和 Windows 启动验收说明。

格式、默认及无桌面编译检查、核心测试、27 项发布测试、文档和差异检查通过。Windows 编译打包及实际启动待远端验证。
```

</details>

- **fix(windows): 恢复客户区顶栏空白的原生拖窗** · [8664033](https://github.com/realmx/MantaSH/commit/8664033c5024d51f9ec22b537c44ed896a38fbe7)

<details>
<summary>完整提交说明</summary>

```text
为 Windows 顶栏空白注册 WindowControlArea::Drag，使 Win32 命中测试返回 HTCAPTION。保留原生系统按钮，通过既有遮挡命中区隔离标签排序、关闭按钮、固定操作和弹窗。同步设计与开发说明。

cargo fmt、默认及无桌面 cargo check、无桌面 cargo test、文档及差异检查通过。Windows 实机拖窗、双击、贴靠和交互回归尚未验证。
```

</details>

- **fix(release): 使用 Git 推送创建版本标签并保留拒绝原因** · [ef23399](https://github.com/realmx/MantaSH/commit/ef233991b6960e8e63a82da36bf15c8c0e921a72)

<details>
<summary>完整提交说明</summary>

```text
使用 checkout 配置的 GITHUB_TOKEN 非强制推送目标源码 SHA，远端接受后才创建本地 tag，避免 Git References API 的笼统 422 阻断诊断。保留显式 Release 派发和已有标签不可覆盖约束。

补充真实本地 Git 远端创建及拒绝覆盖测试，同步发布文档和命令断言。发布相关 Python 测试、Rust 基础检查、文档检查及差异检查通过；远端行为将在 Actions 验证。
```

</details>

- **fix(ui): 修复 Windows 原生窗口操作并调整主题和版本展示** · [4f70d89](https://github.com/realmx/MantaSH/commit/4f70d89caaac1f4a60ccba28535aff339fad75f7)

<details>
<summary>完整提交说明</summary>

```text
Windows 使用系统原生标题栏，客户区保留标签工具行，移除重复的自绘窗口控制按钮，继续使用现有关闭确认流程。

对调白天和黑夜主题切换图标，将设置弹窗版本号移至 footer 左侧，中文按版本x.x.x显示，并同步用户、设计和开发文档。

验证：cargo fmt --all -- --check、cargo check --locked --all-targets、cargo check --locked --no-default-features --all-targets、cargo test --locked --no-default-features --all-targets、python3 scripts/check_docs.py、git diff --check 均通过。Windows 构建和实机操作、原生窗口视觉验收尚未执行。
```

</details>

- **docs: 记录初始发布下载与 Homebrew 安装验证** · [44b0674](https://github.com/realmx/MantaSH/commit/44b0674dc06808f781186efa5e56f2eed2bdf113)

<details>
<summary>完整提交说明</summary>

```text
记录 v1.0.0 五包及校验文件匿名下载、SHA-256 和 DMG 校验结果，补充 macOS arm64 强制下载与重装、应用版本架构签名及二进制一致性证据。明确 Gatekeeper 拒绝状态，以及 macOS 手动放行启动和 Windows 实机验收尚未完成。
```

</details>

- **chore: 初始化 MantaSH 项目与发布配置** · [ffe9d05](https://github.com/realmx/MantaSH/commit/ffe9d05bf8875b9a448965c0f40c5c5ec44cb543)

<details>
<summary>完整提交说明</summary>

```text
基于现有源码建立全新 master 历史，包含终端、SSH、SFTP、测试、文档与 macOS/Windows 发布流程。统一 Homebrew DMG 下载源为 realmx/MantaSH，支持仅授权 tap 写入的 SSH 部署密钥，并说明仓库重建后的下载与安装验证流程。
```

</details>

<!-- END AUTO-GENERATED COMMIT LOG -->
