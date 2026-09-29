# 第三方声明

MantaSH 自有代码、品牌与文档使用 GPL-3.0-or-later，完整文本见 [LICENSE](LICENSE)。第三方依赖仍遵循各自许可，不因使用本项目许可证而改变其授权。

| 依赖/资源 | 用途 | 项目来源 |
|---|---|---|
| GPUI | 原生窗口、文本和绘制 | https://github.com/zed-industries/zed |
| GPUI Component / Assets | 输入、编辑、焦点与 Lucide 图标资源 | https://github.com/longbridge/gpui-component |
| alacritty_terminal | ANSI 解析及终端网格 | https://github.com/alacritty/alacritty |
| portable-pty | PTY / ConPTY | https://github.com/wezterm/wezterm |
| russh / russh-sftp | SSH 和 SFTP | https://github.com/Eugeny/russh / https://github.com/AspectUnk/russh-sftp |
| encoding_rs | 文本转码 | https://github.com/hsivonen/encoding_rs |
| rusqlite / SQLite | 元数据存储 | https://github.com/rusqlite/rusqlite / https://sqlite.org |
| RustCrypto aes-gcm | 本地凭据认证加密 | https://github.com/RustCrypto/AEADs |

精确版本、许可证表达式及源地址由锁定的 Cargo 元数据生成，见 [完整依赖许可清单](docs/dependency-licenses.csv)。发布时按该清单随分发提供需要保留的许可证、版权声明和对应源代码义务。

系统字体由用户平台提供。MantaSH SVG 与位图资产为本工程原创几何图形。OpenSSH sftp-server 仅在测试中调用系统安装的程序，未随项目复制其二进制。

Lucide 图标及其 Feather 来源的完整授权文本另存于 [assets/LUCIDE-LICENSE](assets/LUCIDE-LICENSE)，与组件库代码许可分别保留。

品牌生成脚本使用 Pillow，版本在 requirements-tools.txt 中固定；项目来源为 https://github.com/python-pillow/Pillow，许可为 MIT-CMU。该工具不属于原生应用运行依赖。
