# 文档索引

按任务选择入口；源码版本与发布方式以[发布文档](release.md)为准，功能状态和验收范围以[交付状态](delivery-status.md)与[验收记录](acceptance.md)为准。

## 使用与排障

- [项目主页与安装](../README.md) / [English README](../README.en.md)：定位版本、包与 Homebrew 安装命令。
- [功能与范围](features.md)：支持的平台、能力和不包含的功能。
- [用户手册](user-guide.md)：连接、终端、文件、编辑、传输、历史、监控与快捷键。
- [自动更新](updates.md)：版本检查、确认/取消、下载校验、安装与分阶段验收。
- [排障](troubleshooting.md)：连接、凭据、文件、数据、平台问题的排查。
- [macOS](macos.md) / [Windows](windows.md)：平台调试与人工验证边界。

## 开发与发布

- [贡献指南](../CONTRIBUTING.md)：修改、检查、提交与证据要求。
- [开发与验证](development.md)：代码约定、测试命令和隔离原生 QA 入口。
- [架构与数据流](architecture.md)：模块、会话归属与后台事件。
- [数据、凭据与格式](data.md)：SQLite、加密密码库、CSV、编码和恢复。
- [视觉与交互](design.md)：界面尺寸、颜色和组件规则。
- [能力与接口映射](implementation.md) / [终端机制](terminal-reference.md)：实现边界及终端参考。
- [发布与版本](release.md)：tag、构建目标、Homebrew tap、签名状态和重跑。
- [AI 开发](ai-development.md) / [工程规范](../AGENTS.md)：自动化协作者约束。

## 状态、资源与许可

- [交付状态](delivery-status.md) / [验收记录](acceptance.md)：区分代码接入、构建和实机证据。
- [更新日志](../CHANGELOG.md)：已发布版本摘要。
- [品牌设计](brand-philosophy.md)：标识依据和图标生成约定。
- [依赖许可清单](dependency-licenses.csv) / [第三方声明](../THIRD_PARTY.md) / [项目许可证](../LICENSE)：授权来源与分发义务。

文档描述当前实现；修改用户行为、数据、快捷键、平台行为或产物时同步维护相应页面，不把自动化构建冒充平台实机验收。
