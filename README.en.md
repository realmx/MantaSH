# MantaSH

[中文](README.md)

**MantaSH** is a native terminal workspace built with Rust and GPUI. It brings local Shells, SSH, remote files, editing and Linux host monitoring into one window.

[![Checks](https://github.com/realmx/mantash/actions/workflows/ci.yml/badge.svg)](https://github.com/realmx/mantash/actions/workflows/ci.yml) [![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)

## Install

Install on macOS with Homebrew. Repeating the command when the latest version is installed only reports that no upgrade is needed; update with `brew upgrade --cask mantash`.

```sh
brew install realmx/taps/mantash --cask
```

Alternatively, download the package for your architecture from [GitHub Releases](https://github.com/realmx/MantaSH/releases). On macOS, open the DMG and drag the app into Applications; on Windows, run the installer.

| Platform | Architecture | Release package |
|---|---|---|
| macOS | Apple Silicon | `MantaSH-X.Y.Z-macos-arm64.dmg` |
| macOS | Intel | `MantaSH-X.Y.Z-macos-x64.dmg` |
| Windows | x86 | `MantaSH-X.Y.Z-windows-x86-setup.exe` |
| Windows | x64 | `MantaSH-X.Y.Z-windows-x64-setup.exe` |
| Windows | ARM64 | `MantaSH-X.Y.Z-windows-arm64-setup.exe` |

`X.Y.Z` is the release tag version; every package has a `.sha256`. macOS packages are ad-hoc signed and not notarized. Gatekeeper may block first launch: verify the source and use the system's Open Anyway steps in the [user guide](docs/user-guide.md), without disabling security checks. Windows installers are unsigned and may prompt SmartScreen; Windows installation and runtime have not been verified on hardware. No portable ZIP is provided.

## Capabilities

- Local PTY terminals, tabs and Shell integration; each local tab supports up to five horizontal, vertical or mixed panes. SSH tabs cannot be split.
- SSH password authentication, host-fingerprint verification, reconnection and locally encrypted credentials; searchable profiles with sorting, cloning, multiple selection and CSV import/export.
- SFTP browsing, copying, renaming, deletion, uploads, downloads and background transfers; text editing up to 8 MB with strict encoding, session drafts and undoable basic formatting.
- Shared SSH command history with search, copy, fill and explicit execution; Linux resource, process and port details, with confirmation before terminating a process.
- Chinese and English, light/dark themes, system appearance, fonts, workspace restoration and updates that download, verify and install after confirmation.

See [features and scope](docs/features.md) and the [user guide](docs/user-guide.md) for all entry points and limits. SSH uses password authentication; legacy RSA host-key compatibility is configured, with real legacy-server connections still awaiting validation. Directory uploads, automatic transfer resumption and editor-draft recovery after restart are not available.

## Get Started

The first launch opens a local Shell; use the plus button for another local tab. Open Connections to add an SSH profile, and verify its host fingerprint through a trusted channel before connecting.

Profiles support five-column CSV import/export: `name,host,port,username,password`. Exported passwords are plaintext, so treat the CSV as sensitive. On restart, local tabs start new Shells, SSH requires manual reconnection, and editor drafts are not retained. See the [user guide](docs/user-guide.md).

## Screenshots

These are full native macOS debug-window captures from an isolated data directory. SSH/SFTP uses a real loopback test server; ports and temporary paths belong to that fixture, with no real credentials.

![Local terminal and split panes](assets/screenshots/local-workspace.png)

![SSH file workspace](assets/screenshots/ssh-workspace.png)

![SSH online editor](assets/screenshots/ssh-editor.png)

![SSH transfer review](assets/screenshots/ssh-transfer.png)

![Settings and version](assets/screenshots/settings-version.png)

## Development and Releases

Building from source requires Rust 1.89 or newer and the native C/C++ toolchain for your platform; the first build downloads dependencies. See [development](docs/development.md) for platform setup.

```sh
git clone https://github.com/realmx/MantaSH.git
cd MantaSH
cargo run --locked
```

Required checks, targeted tests and isolated native QA are documented in [development](docs/development.md#必做检查). `Cargo.toml` remains the source version baseline. Source changes pushed to `master` trigger version tagging and release dispatch; documentation-only updates are skipped. See the [release guide](docs/release.md) for version rules, signing and packaging.

## Documentation and License

The [documentation index](docs/README.md) links the user guide, development, architecture, data, platform and acceptance records. See also the [contribution guide](CONTRIBUTING.md). MantaSH is licensed under [GPL-3.0-or-later](LICENSE); third-party licenses are listed in the [third-party notices](THIRD_PARTY.md).
