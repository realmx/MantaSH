# 贡献指南

先从[文档索引](docs/README.md)确认功能范围和维护入口，阅读 [AGENTS.md](AGENTS.md) 的工程规则。开始修改前检查工作区已有改动与任务授权，不覆盖、回滚或删除无关文件；沿用原生 Rust/GPUI、服务和事件分层。

## 修改与文档维护

用户行为、数据、快捷键、平台行为或发布产物改变时，同步更新对应文档。文档描述当前实现；具体机制只在所属文档维护，其余页面链接引用。

| 改动内容 | 维护入口 |
|---|---|
| 功能范围、入口和快捷键 | [功能说明](docs/features.md)、[用户手册](docs/user-guide.md)；概览变化同步根 README |
| 模块、异步归属、认证与后台边界 | [架构](docs/architecture.md)；工程约束变化同步 [AGENTS.md](AGENTS.md) |
| 持久化、CSV、凭据与迁移 | [数据文档](docs/data.md) |
| 控件、布局、主题与品牌资源 | [设计规范](docs/design.md)、[品牌理念](docs/brand-philosophy.md)；新增界面文案进入 `src/ui/i18n.rs` |
| 构建、测试、QA 和依赖 | [开发指南](docs/development.md)、[第三方说明](THIRD_PARTY.md)及[依赖清单](docs/dependency-licenses.csv) |
| 平台差异、故障与验证证据 | [macOS](docs/macos.md)、[Windows](docs/windows.md)、[故障排查](docs/troubleshooting.md)、[验收说明](docs/acceptance.md) |
| 发布与自动更新 | [发布文档](docs/release.md)、[自动更新](docs/updates.md) |

网络、文件、数据库和凭据操作放在后台，异步结果按固定 Owner/request/revision 复核；主机身份确认先于凭据读取和发送。UI 使用统一控件包装层和 `theme::SPACE_*`，核对中英文与字号范围；保存失败保留草稿，不静默丢弃用户数据。

## 验证与提交

运行[必做检查](docs/development.md#必做检查)，再按变更选择相关 Rust 测试、原生 QA 和平台检查。正常窗口、无输入 draw、程序化键盘/指针和人工验收分别记录；Windows 构建成功不代表实机通过，禁止创建或运行本地 Linux VM/容器。

默认分支为 `master`，开发分支可用 `codex/` 前缀。提交正文说明实际修改及原因；交付报告列出执行的命令、结果、未执行检查和剩余限制，不把准备失败或请求接受写成验证通过。版本与发布触发规则见[发布文档](docs/release.md)。

每次 push 到 `master` 后，Actions 自动把全部可达提交的标题、正文、日期和链接写入 `CHANGELOG.md` 的生成区；人工维护上方功能摘要，勿改写生成区。普通提交、合并和文档更新均记录，仅跳过明确标记且只改日志的机器人同步提交。提交正文须足以说明实际改动，自动日志不能补出未写入提交的事实。后续开发先拉取日志同步提交；本地生成、检查和失败重跑见[更新日志自动生成](docs/release.md#更新日志自动生成)。

不要提交密码、私钥、用户数据库、QA 控制状态、构建缓存或临时日志。删除只限用户已授权的目标，保留其他协作者和用户的工作。
