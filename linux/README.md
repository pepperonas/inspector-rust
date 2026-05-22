# Inspector Rust — Linux (Ubuntu)

Native Tauri 2 shell for Ubuntu and other Debian-based distros. Core logic lives in `core/rust-lib`; this directory is only the Linux bundle (config, icons, capabilities).

## Quick start

From the repository root:

```bash
bash scripts/install-linux.sh   # apt deps + Node 20 + pnpm + Rust stable
source "$HOME/.cargo/env"       # if rustup was installed by the script
pnpm dev:linux                  # development (tray + Ctrl+Shift+V popup)
pnpm build:linux                # release .deb + AppImage
```

Install artifacts:

- Binary: `linux/src-tauri/target/release/inspector-rust`
- Bundle: `target/release/bundle/deb/InspectorRust_<ver>_amd64.deb`

Install:

```bash
sudo dpkg -i target/release/bundle/deb/InspectorRust_*_amd64.deb
```

## What works on Linux

| Feature | Status |
| -------- | ------ |
| Clipboard history (`Ctrl+Shift+V`) | Yes |
| Snippets, notes, backup, calculator, colors | Yes |
| AES-256 DB encryption + Secret Service keyring | Yes (keyfile fallback) |
| Global shortcuts + system tray | Yes (X11; Wayland may need compositor support) |
| Paste into focused app (`Ctrl+V` via enigo) | Yes |
| ML background cutout (ONNX) | Yes (offline) |
| Region screenshot (`Ctrl+Shift+S`) | Yes with **scrot** (X11) or **grim+slurp** (Wayland) |
| Screen OCR (`Ctrl+Shift+O`) | Requires **tesseract** + `tesseract-ocr-eng` (German optional: `tesseract-ocr-deu`) |
| In-app eyedropper | Not yet (macOS/Windows only) |
| Text expander in-place (AX/UIA) | Keystroke/clipboard fallback only |

Data path: `~/.local/share/InspectorRust/history.db`

## System packages

Minimum (also used in CI):

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf \
  libxdo-dev libxcb-shape0-dev libxcb-xfixes0-dev build-essential pkg-config libssl-dev
```

Recommended for full feature set:

```bash
sudo apt-get install -y scrot tesseract-ocr tesseract-ocr-eng tesseract-ocr-deu
# Wayland only:
sudo apt-get install -y grim slurp
```

## Desktop environment notes

- **X11**: Global shortcuts and `scrot -s` region capture usually work out of the box.
- **Wayland**: Install `grim` and `slurp` for region capture. Global shortcuts depend on the compositor; if `Ctrl+Shift+V` does not fire, try an X11 session or check compositor shortcut policies.
- **Clipboard on Wayland**: If the log shows `ext-data-control` / `wlr-data-control` is missing, the app falls back to the X11 clipboard bridge. For full Wayland clipboard sync, use a compositor that supports those protocols, or run under an X11/XWayland session.
- **Autostart**: Uses the Tauri autostart plugin (typically `~/.config/autostart/`).

## Troubleshooting

| Problem | Fix |
| -------- | ----- |
| `webkit2gtk` not found | Run `scripts/install-linux.sh` or install `libwebkit2gtk-4.1-dev` |
| `cargo` / edition errors | `rustup default stable` (need Rust ≥ 1.77) |
| Region capture fails | Install `scrot` (X11) or `grim`+`slurp` (Wayland) |
| OCR shortcut errors | `sudo apt install tesseract-ocr tesseract-ocr-eng` (optional German: `tesseract-ocr-deu`) |
| Tray icon missing | `libayatana-appindicator3-dev` + log out/in |

## Related docs

- [CONTRIBUTING.md](../CONTRIBUTING.md) — adding IPC commands and platform shells
- [docs/encryption.md](../docs/encryption.md) — Secret Service + keyfile fallback
- [docs/text-expander.md](../docs/text-expander.md) — expander modes on Linux
