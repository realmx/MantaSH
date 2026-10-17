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

These five full native macOS window captures all use the dark theme. They use isolated data and a real loopback SSH/SFTP server; ports, temporary paths and files belong to the test environment, with no production credentials. The window is 1280×800 and the images retain 2× Retina resolution. See the [acceptance record](docs/acceptance.md#readme-原生截图) for the captured build and scope.

![Local terminal: dark theme and mixed split panes](assets/screenshots/local-workspace.png)

![SSH file workspace: dark theme and inline file actions](assets/screenshots/ssh-workspace.png)

![Online editor: dark theme, line numbers, formatting and saving](assets/screenshots/ssh-editor.png)

![Upload review: file sizes and overwrite option](assets/screenshots/ssh-transfer.png)

![Settings: grouped switches and selected theme and language](assets/screenshots/settings-version.png)

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
