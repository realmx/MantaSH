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

`vendor/gpui-component` 保留上游 0.5.1 源码及 [Apache-2.0 许可证](vendor/gpui-component/LICENSE-APACHE)，通过 Cargo 本地 patch 提供编辑事务、撤销分组和选区/滚动恢复扩展，并修正按钮悬停前景色遵循其 variant；修改范围见 [补丁说明](vendor/gpui-component/MANTASH-PATCH.md)。版本未升级，Assets 依赖仍使用锁定的上游包。依赖清单中该包的 `MantaSH workspace` 表示本地路径来源，不表示其版权或许可证归属改变。

系统字体由用户平台提供。MantaSH SVG 与位图资产为本工程原创几何图形。OpenSSH sftp-server 仅在测试中调用系统安装的程序，未随项目复制其二进制。

Lucide 图标及其 Feather 来源的完整授权文本另存于 [assets/LUCIDE-LICENSE](assets/LUCIDE-LICENSE)，与组件库代码许可分别保留。

品牌生成脚本使用 Pillow，版本在 requirements-tools.txt 中固定；项目来源为 https://github.com/python-pillow/Pillow，许可为 MIT-CMU。该工具不属于原生应用运行依赖。
