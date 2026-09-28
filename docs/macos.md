# macOS 安装、调试与验收

正式版可从 [GitHub Releases](https://github.com/realmx/MantaSH/releases) 下载对应架构的 DMG，也可使用 Homebrew：

```sh
brew install realmx/taps/mantash --cask
```

应用采用 ad-hoc 签名、未经 Apple 公证，首次打开可能被 Gatekeeper 拦截。先核对下载来源与校验和，再按[用户手册](user-guide.md#macos-无法直接打开)使用系统“仍要打开”；不要关闭系统安全检查。发布产物、版本与更新方式见[发布文档](release.md)。

## 源码环境

安装 Rust 1.88 或兼容工具链及 Xcode Command Line Tools（或 Xcode）。在项目根目录确认：

```sh
rustc --version
cargo --version
xcode-select -p
```

GPUI 使用运行时 Metal 着色器编译，调试构建无需单独的 `metal` 命令。初次构建需要下载锁定依赖；缓存齐全后可按需使用 `--offline`。

## 调试运行

```sh
sh scripts/check-macos.sh
cargo run --locked
```

也可用 `sh scripts/check-macos.sh --run` 在检查后启动。上述命令不生成发布 DMG；若仅运行已编译的 debug 程序，可执行 `./target/debug/mantash`。源码更新后必须重新编译并正常退出旧实例，运行中的窗口不会自动替换。

使用独立数据目录复现问题：

```sh
MANTASH_DATA_DIR="$TMPDIR/mantash-manual-check" cargo run --locked
```

此目录与日常数据隔离，但仍连接真实 Shell/SSH 和文件系统，不要用生产凭据做无关测试。普通使用无需辅助功能权限；自动注入系统键鼠事件需另行授予权限。[开发与验证](development.md#原生视图驱动debug-qa)说明隔离 QA 入口及证据边界。

## 人工回归

macOS 输入法、分屏、文件和窗口操作已有用户人工确认，范围见[验收记录](acceptance.md)。后续版本仍应核对中文输入法组合/选词、剪贴板、Vim/less/top、窗口缩放与外接显示器，以及真实指针和系统文件选择器。程序化输入、截图和构建成功不能替代这些操作证据。
