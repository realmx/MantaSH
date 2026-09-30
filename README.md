# MantaSH

[English](README.en.md)

**MantaSH** 是基于 Rust 和 GPUI 的原生终端工作台，将本地 Shell、SSH、远程文件、编辑和 Linux 主机监控集中在一个窗口中。

[![Checks](https://github.com/realmx/mantash/actions/workflows/ci.yml/badge.svg)](https://github.com/realmx/mantash/actions/workflows/ci.yml) [![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

## 安装

macOS 可用 Homebrew 安装；已安装最新版本时重复执行只会提示无需升级。更新使用 `brew upgrade --cask mantash`。

```sh
brew install realmx/taps/mantash --cask
```

也可从 [GitHub Releases](https://github.com/realmx/MantaSH/releases) 下载对应架构的包。macOS 打开 DMG 后将应用拖入 Applications；Windows 运行安装器。

| 平台 | 架构 | 发布包 |
|---|---|---|
| macOS | Apple Silicon | `MantaSH-X.Y.Z-macos-arm64.dmg` |
| macOS | Intel | `MantaSH-X.Y.Z-macos-x64.dmg` |
| Windows | x86 | `MantaSH-X.Y.Z-windows-x86-setup.exe` |
| Windows | x64 | `MantaSH-X.Y.Z-windows-x64-setup.exe` |
| Windows | ARM64 | `MantaSH-X.Y.Z-windows-arm64-setup.exe` |

`X.Y.Z` 对应发布 tag；每个包附有 `.sha256`。macOS 包仅 ad-hoc 签名、未经公证，首次打开可能被 Gatekeeper 拦截；核对来源后按[用户手册](docs/user-guide.md)使用系统“仍要打开”，不要关闭安全检查。Windows 安装器未签名，SmartScreen 可能提示确认；Windows 实机安装与运行尚未验收。不提供便携 ZIP。

## 核心能力

- 本地 PTY、多标签与 Shell 历史；每个本地标签最多混合分屏为 5 个窗格，SSH 不分屏。
- SSH 密码认证与主机指纹确认；确认主机后才发送密码，认证成功后在本机 AES-256-GCM 加密保存，不设主密码、不用系统钥匙串。
- SFTP 文件浏览与传输、远程文本编辑（普通文本不超过 8 MB，保存时最后覆盖），以及 SSH 标签共享历史。
- Linux 远端资源、进程与端口监控；中英文、明暗主题、字体设置和工作区恢复。

不支持私钥认证、仅提供 RSA 主机密钥的服务器或目录上传入口。其他边界见[功能与范围](docs/features.md)。

## 开始使用

启动后进入本地 Shell；点击加号新建本地标签。通过“连接”添加 SSH 资料，连接前请经可信渠道核对主机指纹。

连接资料支持 `name,host,port,username,password` 五列 CSV 导入导出；导出密码为明文，请按敏感文件处理。重启后本地标签启动新的 Shell，SSH 需手动重连，编辑草稿不保留。详见[用户手册](docs/user-guide.md)。

## 界面预览

以下为隔离数据目录中的完整 macOS 原生调试窗口截图；SSH/SFTP 使用真实回环测试服务。画面中的端口和临时路径仅属于测试环境，不含真实凭据。

![本地终端与分屏](assets/screenshots/local-workspace.png)

![SSH 文件工作区](assets/screenshots/ssh-workspace.png)

![SSH 在线编辑](assets/screenshots/ssh-editor.png)

![SSH 传输核对](assets/screenshots/ssh-transfer.png)

![设置与版本](assets/screenshots/settings-version.png)

## 开发与发布

源码运行需要 Rust 1.88 和对应平台的 C/C++ 工具链；首次构建需要下载依赖。平台准备见[开发文档](docs/development.md)。

```sh
git clone https://github.com/realmx/MantaSH.git
cd MantaSH
cargo run --locked
```

基础检查：

```sh
cargo fmt --all -- --check
cargo test --locked --no-default-features --all-targets
cargo check --locked --no-default-features --all-targets
python3 scripts/check_docs.py
```

`Cargo.toml` 是源码版本基线，major/minor 由维护者手工决定；`master` 更新若只涉及根目录 Markdown、`docs/` 中的 Markdown 或 `assets/screenshots/` 截图，不创建 tag 或 Release。出现其它变更时，版本 workflow 按上个 tag 至当前提交的差异创建一个 `vX.Y.Z` tag，不提交自动增版；Release 构建临时同步版本元数据。已发布版本以 tag 和 Release 为准；分支构建只提供限时构件。详见[发布文档](docs/release.md)。

## 文档与许可

[文档索引](docs/README.md)汇总用户手册、开发、架构、数据、平台和验收记录；另见[贡献指南](CONTRIBUTING.md)。项目采用 [GPL-3.0-or-later](LICENSE)，第三方许可见[第三方声明](THIRD_PARTY.md)。
