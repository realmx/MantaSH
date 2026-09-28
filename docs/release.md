# 发布与版本

## 版本来源

`Cargo.toml` 是唯一源码版本入口，当前基准为 `1.1.0`；Rust 使用 `CARGO_PKG_VERSION` 显示版本。维护者按 SemVer 手工决定 major/minor 变更。版本 workflow 将最新可达的发布 tag 与本次 `master` 提交比较：仅文档与 README 截图变更不建 tag；其它变更按源码基线或补丁规则为该提交分配 `vX.Y.Z` tag，不向主分支提交自动版本变更。已发布版本以 tag 和 [GitHub Release](https://github.com/realmx/MantaSH/releases) 为准。SQLite 格式和连接 CSV 列独立维护。

Release runner 在编译前仅在本次检出的源码中把 `Cargo.toml`、`Cargo.lock` 和 `docs/dependency-licenses.csv` 同步为 tag 版本；这些改动不推回 `master`。

## 触发与构建范围

| 触发方式 | 版本与结果 |
|---|---|
| `master` push（含 PR 合并） | [version.yml](../.github/workflows/version.yml) 忽略纯文档更新；其它更新创建一个 tag，并在新 tag 上显式派发 Release |
| 外部推送 `v*` tag | 校验 tag 与 Cargo 基准后构建完整 Release |
| 推送 `build/**` 分支 | 仅构建开发产物，不创建 Release |
| 手动运行 `release.yml` | tag ref 构建完整 Release；分支 ref 可选 `all`、`windows-x64`、`macos-arm64` 构建开发产物 |

纯文档指根目录 Markdown、`docs/**/*.md` 与 `assets/screenshots/` 中的 README 图片；`docs/dependency-licenses.csv`、发布脚本、workflow、源码和其它资源均算发布相关变更。对比范围是上个版本 tag 至当前提交的完整文件差异：连续文档提交均不增版，其后的源码更新会一并包含这些文档。重命名同时检查旧路径与新路径；无差异的提交也不建 tag。若尚无版本 tag，首版仍使用 `Cargo.toml` 基准版本。

`GITHUB_TOKEN` 创建的 tag 不依赖 tag push 再触发 workflow，因此版本流程显式派发。已有 tag 或过期的旧 push 不重复分配版本。tag 在发包前创建；构建失败会留下 tag，修复后可重跑同一 tag。开发构建以 `<源码基准>-dev.<短SHA>` 命名，作为 Actions artifacts 保留 14 天；`build/windows-x64`、`build/macos-arm64` 各只构建对应目标，其余 `build/**` 构建全部目标。

Actions 使用 checkout 配置的任务自身 `GITHUB_TOKEN`，通过非强制 Git push 创建指向当前源码提交的 tag，远端接受后才记录本地 tag 并显式派发 Release。推送前记录版本和目标 SHA；Git 服务端拒绝时保留具体错误并使任务失败，不切换凭据、不覆盖已有 tag，也不绕过 ref 保护。若 GitHub 明确返回某个 tag 名因仓库创建规则被保留，版本脚本只跳过该候选补丁版本并尝试下一个；权限、网络或其它拒绝仍直接失败。

合并或直接提交经审核的更新后，推送 `master` 会运行版本判定；包含发布相关变更时才创建下一个公开版本。推送前确认变更和基准版本：

```sh
git push origin master
```

## 正式产物

正式安装包面向 macOS 和 Windows，不提供便携 ZIP。每个安装包附一个同名 `.sha256`：

| 目标 | 文件名 |
|---|---|
| macOS arm64 | `MantaSH-X.Y.Z-macos-arm64.dmg` |
| macOS x64 | `MantaSH-X.Y.Z-macos-x64.dmg` |
| Windows x86 | `MantaSH-X.Y.Z-windows-x86-setup.exe` |
| Windows x64 | `MantaSH-X.Y.Z-windows-x64-setup.exe` |
| Windows ARM64 | `MantaSH-X.Y.Z-windows-arm64-setup.exe` |

macOS 应用使用 ad-hoc 签名，DMG 未经 Apple 公证，不要求或读取 Developer ID/公证 secrets；Windows 使用 [Inno Setup](../packaging/windows/MantaSH.iss) 打包，未做代码签名。工作流在 macOS runner 核对 DMG 校验和、架构、版本及签名；Windows runner 产出安装器；发布前要求五包及其校验文件全部存在并通过 SHA-256。任一目标失败，publish job 不执行。

macOS Gatekeeper 可能阻止首次启动，Windows SmartScreen 可能提示确认。先核对下载来源及校验和，macOS 按[用户手册](user-guide.md#macos-无法直接打开)使用系统“仍要打开”，不使用 `xattr` 等方式关闭安全检查。Actions 构建通过不等于 Windows 实机安装或运行验收通过。

## Homebrew tap

macOS cask 位于独立的 [realmx/homebrew-taps](https://github.com/realmx/homebrew-taps) 仓库 `Casks/mantash.rb`，下载源为本项目 Release 的 DMG：

```sh
brew install realmx/taps/mantash --cask
brew upgrade --cask mantash
```

主仓库 Actions 变量 `MANTASH_HOMEBREW_TAP=realmx/homebrew-taps` 配合 secret `TAP_SSH_PRIVATE_KEY`（tap 专用、允许写入的 SSH deploy key）时，Release 成功后更新 cask；也兼容有 tap 写权限的 `TAP_GITHUB_TOKEN`。部署密钥只授权独立 tap，不使用个人登录令牌。缺配置会跳过，更新失败也不撤销已发布的 Release。凭据不提交到仓库。Cask 不绕过 Gatekeeper。

下载地址固定为 `https://github.com/realmx/MantaSH/releases/download/vX.Y.Z/MantaSH-X.Y.Z-macos-<arm64或x64>.dmg`；cask 的 SHA-256 来自同一次发布的实际 DMG，并核对旁附校验文件。仓库重建后须重新配置 Actions 变量和部署密钥，发布完整五目标附件后再验证 tap。若本机已装同版本，使用 `brew fetch --cask --force realmx/taps/mantash` 和 `brew reinstall --cask realmx/taps/mantash` 验证新附件，不能把旧缓存或“已安装”提示当作下载及安装通过。

## 本地检查

不跨平台模拟 Windows 安装器或 macOS DMG。发布前可运行：

```sh
cargo fmt --all -- --check
cargo test --locked --no-default-features --all-targets
cargo check --locked --no-default-features --all-targets
python3 scripts/release_version.py validate --version 1.0.0
python3 -m py_compile scripts/package_release.py scripts/release_version.py scripts/release_matrix.py scripts/release_notes.py scripts/update_homebrew_cask.py
python3 -m unittest discover -s tests -p 'test_release_*.py'
python3 scripts/check_docs.py
```

macOS 可在 `cargo build --locked --release --target aarch64-apple-darwin --bin mantash` 后运行下列本机打包检查；这不代表 x64、Windows 或 GitHub runner 已通过。Windows 打包需目标二进制和 Inno Setup 6。

```sh
python3 scripts/package_release.py --target aarch64-apple-darwin \
  --binary target/aarch64-apple-darwin/release/mantash \
  --version 1.0.0 --output-dir dist
```

不要提交 `.pi/`、`.zcode/`、`target/`、`dist/`、数据库、凭据或临时日志；平台人工结果单独登记在[验收状态](acceptance.md)。

## 失败与重跑

```sh
gh run list --workflow release.yml
gh run rerun <RUN_ID>
# 已有 tag 且需要重新派发时：
gh workflow run release.yml --ref vX.Y.Z --field platform=all
```

版本校验失败时检查 tag 来源与源码基准，不强制覆盖 tag。构建、签名校验或附件检查失败时查看对应 job；同名 tag 重跑会更新其已有 Release 附件，发布新版本应创建新 tag。开发构件过期后重新触发分支构建即可。
