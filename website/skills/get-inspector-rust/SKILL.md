---
name: get-inspector-rust
description: Download and install the newest Inspector Rust — the free, open-source (MIT) clipboard manager, text expander and command palette for macOS (Apple silicon), Windows and Linux. Use when someone asks for the app, its latest version, a download link for their system, or how to get past the first-launch warning on macOS or Windows.
license: MIT
---

# Get Inspector Rust

Free, open-source (MIT) desktop utility for macOS, Windows and Linux. Ctrl+Space opens a popup with a searchable, encrypted clipboard history, a text expander and 66 search-bar commands: calculator and unit converter, translation, weather, screenshots with OCR and annotation, screen recording, colour picker, system and audio tools. Data stays in a local SQLite database with AES-256-GCM encryption at rest; there is no account and no telemetry.

## 1. Find the newest release

`GET https://inspector-rust.celox.io/latest.json` returns `version`, `published`, `notes` and `assets[]`, each with `target`,
`name`, `url`, `size` (bytes) and `sha256`. It is refreshed from GitHub Releases every 15 minutes. On the
page itself, browsers with WebMCP expose the same data as the tools `get_latest_release`,
`get_download_url` and `get_checksums`.

## 2. Download

- Stable link, always the newest file for the visitor's platform: <https://inspector-rust.celox.io/download>
- macOS: <https://inspector-rust.celox.io/download/macos> — macOS 11+ · Apple silicon
- Windows: <https://inspector-rust.celox.io/download/windows> — Windows 10/11 · x64 · installer
- Windows .exe: <https://inspector-rust.celox.io/download/windows-exe> — Windows 10/11 · x64 · no install
- Linux .deb: <https://inspector-rust.celox.io/download/linux-deb> — Ubuntu 24.04+ / Debian 13 · x86-64
- AppImage: <https://inspector-rust.celox.io/download/linux-appimage> — Linux x86-64

## 3. Verify

- The file's SHA-256 must equal the matching `assets[].sha256` in `latest.json`.

- Compare the file with the SHA-256 shown here: `shasum -a 256` on macOS and Linux, `Get-FileHash` in PowerShell.

## 4. Install

1. DMG for Macs with Apple silicon, MSI installer or portable exe for Windows, .deb or AppImage for Linux.
2. On macOS, move the app to Applications and run `xattr -dr com.apple.quarantine /Applications/InspectorRust.app` once. On Windows, SmartScreen may ask — choose *More info → Run anyway*.
3. The popup opens wherever you are. On macOS, grant Accessibility once so pasting and the expander can type for you.

## Limits

- macOS builds are Apple silicon only and not notarized; the first launch needs one Terminal command.
- Some features are macOS-only (for example the network monitor and touchpad gestures).
- Linux: global shortcuts may need to be bound in the desktop settings under Wayland.

More: [product page](https://inspector-rust.celox.io/) · [Markdown version](https://inspector-rust.celox.io/index.md) · [changelog](https://inspector-rust.celox.io/changelog.md) · [source](https://github.com/pepperonas/inspector-rust)
