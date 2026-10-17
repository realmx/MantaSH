# 文档索引

MantaSH 的文档按使用、工程维护和发布验收组织。功能以当前源码实现为准，已发布版本以 [GitHub Releases](https://github.com/realmx/MantaSH/releases) 为准；构建通过与平台实机验收分别记录。

## 使用 MantaSH

| 文档 | 内容 |
|---|---|
| [项目首页](../README.md) / [English](../README.en.md) | 安装包、Homebrew、原生界面截图与源码启动 |
| [功能与范围](features.md) | 完整功能导航、平台范围和产品限制 |
| [用户手册](user-guide.md) | 本地终端、SSH、连接库、CSV、文件传输、编辑、历史、监控、设置、恢复与快捷键 |
| [自动更新](updates.md) | 自动与手动检查、下载校验、安装条件、取消与失败处理 |
| [排障](troubleshooting.md) | 连接、凭据、编码、文件、监控和本地数据问题 |
| [macOS](macos.md) / [Windows](windows.md) | 平台环境、原生窗口行为及实机检查清单 |

## 开发与维护

| 文档 | 内容 |
|---|---|
| [贡献指南](../CONTRIBUTING.md) | 修改流程、提交与验证要求 |
| [工程规范](../AGENTS.md) | 产品边界、异步与安全规则、自动化协作约束 |
| [架构与数据流](architecture.md) | 模块职责、终端机制、会话归属、后台事件与持久化 |
| [开发与验证](development.md) | 工具链、必做检查、专项测试与隔离原生 QA |
| [数据与凭据](data.md) | SQLite、历史迁移、密码库、CSV、编码、文件完整性和恢复 |
| [视觉与交互](design.md) | 布局、组件、颜色、弹窗尺寸、焦点和平台窗口规则 |

## 发布、证据与资源

| 文档 | 内容 |
|---|---|
| [发布与版本](release.md) | 自动版本、构建矩阵、安装包、校验、Homebrew 与失败重跑 |
| [验收记录](acceptance.md) | 已运行检查、历史人工确认和尚未验证的范围 |
| [更新日志](../CHANGELOG.md) | 功能摘要与自动生成的完整逐次提交记录 |
| [品牌设计](brand-philosophy.md) | 标识设计依据 |
| [第三方声明](../THIRD_PARTY.md) / [依赖清单](dependency-licenses.csv) / [许可证](../LICENSE) | 依赖来源、授权与分发要求 |

操作步骤集中在用户手册；数据契约、设计规则、架构和验证方法分别维护在对应页面。修改功能、数据、快捷键、平台行为或发布产物时同步更新相应文档，运行 `python3 scripts/check_docs.py` 检查必需文件、本地链接、章节锚点和界面文案键。
