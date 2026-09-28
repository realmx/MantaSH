# 验收记录

本页只区分实际人工确认、自动化证据与未验证项目；功能清单见[功能与范围](features.md)，当前交付结果见[交付状态](delivery-status.md)。不同构建、环境或输入方式的结果不能互相冒充。

## 已有人工确认

- macOS 原生输入法、本地分屏、文件和窗口操作在用户确认的环境中正常；不自动扩展为所有系统版本、安装方式或边界场景。
- Linux 远端的基本连接、系统概览、进程操作和端口功能在用户确认的主机范围内通过；其它服务器配置另行记录。
- 连接库的单选、Shift/Cmd 增减选区和批量删除已有人工确认。

## 自动化覆盖

[开发与验证](development.md#必做检查)列出基础命令。CI 覆盖格式、核心测试/检查、文档和发布辅助脚本；真实 PTY、回环 SSH/SFTP、加密凭据、编码、SQLite、传输与隔离原生 QA 分别有专项入口。Actions 可生成 macOS DMG 和 Windows 安装器，但构建、代码签名校验和人工安装是不同证据。

发布的 macOS 包是 ad-hoc 签名、未经 Apple 公证，Windows 安装器未代码签名。`v1.0.0`（源码 `ffe9d05`）的五目标 Release 与 Homebrew cask 更新已在 runner 完成；五个安装包及五个校验文件匿名下载均返回 HTTP 200，SHA-256 全部匹配。在 macOS 27.0 arm64 / Homebrew 7.0.6 上执行 `brew fetch --cask --force realmx/taps/mantash`、`brew reinstall --cask realmx/taps/mantash` 成功，应用安装到 `/Applications/MantaSH.app`，版本为 `1.0.0`、架构为 arm64、ad-hoc 签名有效，二进制与公开 DMG 逐字节一致；再次执行标准安装命令提示已是最新版。两个 DMG 均通过磁盘映像校验。本机 `spctl --assess` 返回拒绝，尚未将用户在系统中点击“仍要打开”后的启动记为通过。详见[发布文档](release.md)与[交付状态](delivery-status.md)。

## 尚未验收

- Windows x86/x64/ARM64 的安装/卸载、ConPTY、DPI、输入法、文件操作及完整运行，按[Windows 清单](windows.md#实机检查清单)逐项测试。
- macOS 新发布包经 Gatekeeper 手动放行后的首次启动、物理指针和系统文件选择器，应以实际操作记录；不能用隔离 QA 的事件投递或无输入 `draw` 代替。
- Linux 新主机的进程/端口边界与权限差异不能由既有 fixture 推断通过。

新验收记录须注明提交或 tag、平台架构和系统版本、操作路径、预期与实际结果；失败不含生产凭据、完整用户数据库或敏感命令历史。
