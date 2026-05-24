# Inspector Rust — Linux (Ubuntu)

Native Tauri 2 shell for Ubuntu and other Debian-based distros. Core logic lives in `core/rust-lib`; this directory is only the Linux bundle (config, icons, capabilities).

## Quick start

From the repository root:

```bash
bash scripts/install-linux.sh   # apt deps + Node 20 + pnpm + Rust stable
source "$HOME/.cargo/env"       # if rustup was installed by the script
pnpm dev:linux                  # development (tray + Ctrl+Shift+V popup)
pnpm build:linux                # release .deb + AppImage (AppImage may fail locally)
pnpm build:linux:deb            # .deb only — recommended on Ubuntu (exit 0)
```

Install artifacts:

- Binary: `target/release/inspector-rust`
- Bundle: `target/release/bundle/deb/InspectorRust_<ver>_amd64.deb`

Install (use the **exact version**, not a glob — old `.deb` files can linger in `bundle/deb/`):

```bash
sudo dpkg -i target/release/bundle/deb/InspectorRust_0.25.1_amd64.deb
killall inspector-rust 2>/dev/null; inspector-rust &
```

### Build notes

| Target | Local Ubuntu | CI |
|--------|----------------|-----|
| `.deb` | `pnpm build:linux:deb` | ✅ |
| `.AppImage` | Often fails with `failed to run linuxdeploy` unless `libfuse2` is installed and FUSE works | ✅ (CI installs `libfuse2`) |

If `pnpm build:linux` exits **1** but you see `Bundling InspectorRust_…_amd64.deb` above the AppImage error, the **`.deb` is still valid** — install it, or use `pnpm build:linux:deb` next time.

AppImage troubleshooting: `sudo apt install libfuse2`, then rebuild. In Cursor’s integrated terminal, linuxdeploy sometimes fails for sandbox reasons; use a normal GNOME Terminal for AppImage builds.

## What works on Linux

| Feature | Status |
| -------- | ------ |
| Clipboard history (`Ctrl+Shift+V`) | Yes |
| Snippets, notes, backup, calculator, colors | Yes |
| AES-256 DB encryption + Secret Service keyring | Yes (keyfile fallback) |
| Global shortcuts + system tray | Yes (X11; Wayland may need compositor support) |
| Paste into focused app (`Ctrl+V` via enigo) | Yes |
| ML background cutout (ONNX) | Yes (offline) |
| Region screenshot (`Ctrl+Shift+S`) | Yes — **xdg-desktop-portal** on GNOME Wayland; **grim+slurp** on wlroots; **scrot** on X11 |
| Screen OCR (`Ctrl+Shift+O`) | Same capture path as screenshot + **tesseract** (`tesseract-ocr-eng`; German: `tesseract-ocr-deu`) |
| Color picker (`Ctrl+Shift+C`) | Yes on GNOME Wayland via **xdg-desktop-portal PickColor** (macOS/Windows use native loupe) |
| Text expander hotkey | **Enable** in Settings → pick **Alt+1** preset → **Save & re-register**. On GNOME Wayland the hotkey is registered via gsettings (not in-app). Avoid `Ctrl+Backquote` on German keyboards — it maps to `grave`, not the `<` key (`less`). Expansion uses **AT-SPI** (reads the focused text field over D-Bus) — **not** synthetic Ctrl+Shift+← in the terminal. |

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
# wlroots Wayland (Sway/Hyprland) — not needed on GNOME (uses xdg-desktop-portal):
sudo apt-get install -y grim slurp
# GNOME Wayland needs the portal (usually preinstalled):
sudo apt-get install -y xdg-desktop-portal xdg-desktop-portal-gnome
```

## Desktop environment notes

- **X11**: Global shortcuts and `scrot -s` region capture usually work out of the box.
- **Wayland (GNOME / Cinnamon)**: Tauri global shortcuts often **do not fire** ([upstream issue](https://github.com/tauri-apps/plugins-workspace/issues/3267)). Inspector Rust **registers system shortcuts automatically** on first launch (via `gsettings`) — same keys as documented:

| Action | Shortcut | What happens |
|--------|----------|----------------|
| Open popup | `Ctrl+Shift+V` | `inspector-rust --toggle-popup` |
| OCR region | `Ctrl+Shift+O` | `inspector-rust --ocr` |
| Screenshot region | `Ctrl+Shift+S` | `inspector-rust --screenshot` |
| Pick color | `Ctrl+Shift+C` | `inspector-rust --pick-color` |

Check under **Settings → Keyboard → Custom Shortcuts** (entries named “Inspector Rust — …”).

**Automatic conflict handling (v2):** On first start (and after profile upgrades) Inspector Rust:

1. Scans existing **custom shortcuts** and **GNOME Terminal** copy/paste bindings via `gsettings`.
2. **Never** changes Terminal — copy/paste stay **Ctrl+Shift+C/V**. If an older build wrongly set Ctrl+C/V, Inspector restores Terminal on startup.
3. Picks the first free binding per action (defaults: Ctrl+Shift+V/O/S/C; fallbacks e.g. Ctrl+Alt+… when Terminal or other apps occupy a key).

No manual `ubuntu-terminal-copy-paste-ctrl-cv.sh` required.

**Settings UI:** Open the app → **Settings** → **Linux desktop shortcuts**. The panel rescans conflicts, lists free presets per action, and supports **record mode** (press a combination, verify, then **Save shortcuts**). **Auto-resolve all** runs the same automatic install as first launch.

**CLI / install script:**

```bash
inspector-rust --setup-shortcuts   # force re-scan + install (after .deb upgrade)
bash scripts/install-linux.sh    # runs --setup-shortcuts when the binary is on PATH
```

- **X11**: No extra setup — built-in global shortcuts usually work.
- **KDE Plasma**: Not automated yet; bind shortcuts manually to the commands above.

- **Wayland region capture (GNOME)**: Uses **xdg-desktop-portal** (Ubuntu's native screenshot UI). Do **not** rely on `slurp` on GNOME — it hangs with no UI.
- **Wayland region capture (Sway/Hyprland)**: Install `grim` + `slurp`.
- **Clipboard on Wayland**: If the log shows `ext-data-control` / `wlr-data-control` is missing, the app falls back to the X11 clipboard bridge. For full Wayland clipboard sync, use a compositor that supports those protocols, or run under an X11/XWayland session.
- **Autostart**: Uses the Tauri autostart plugin (typically `~/.config/autostart/`).

## Troubleshooting

| Problem | Fix |
| -------- | ----- |
| `webkit2gtk` not found | Run `scripts/install-linux.sh` or install `libwebkit2gtk-4.1-dev` |
| `cargo` / edition errors | `rustup default stable` (need Rust ≥ 1.77) |
| Region capture fails / nothing on screen | GNOME: ensure `xdg-desktop-portal-gnome` is installed. Sway/Hyprland: install `grim`+`slurp`. X11: `scrot` |
| OCR shortcut errors | `sudo apt install tesseract-ocr tesseract-ocr-eng` (optional German: `tesseract-ocr-deu`) |
| Color picker does nothing | Rebuild with latest Linux portal support; needs `xdg-desktop-portal-gnome` |
| Text expander hotkey dead (Wayland) | Settings → **Enable** expander → pick hotkey (e.g. Alt+1) → **Save & re-register** — gsettings entry is created automatically |
| Expander captures terminal scrollback / huge text | You tested with focus in the **terminal** — use WhatsApp, gedit, or Firefox. Log should show `AT-SPI: read word before cursor: "mfg"` not clipboard fallback |
| Diagnose shows **clipboard** path | Focus a normal text field (not Terminal) before Diagnose; ensure `org.a11y.Bus` is running (`gsettings get org.gnome.desktop.a11y.interface toolkit-accessibility`) |
| Diagnose closes Settings | **Expected** — the popup must hide so Inspector can read the field in the app behind it. Type your abbreviation in another app first, then click Diagnose |
| `Ctrl+Shift+V` does nothing (Wayland) | Restart app once (auto gsettings), or run `bash scripts/install-desktop-shortcuts.sh` after build |
| Conflict with copy/paste (`Ctrl+Shift+C/V`) | Automatic on install/first start; or Settings → Linux desktop shortcuts → **Auto-resolve all** |
| Tray icon missing | `libayatana-appindicator3-dev` + log out/in |

## Related docs

- [CONTRIBUTING.md](../CONTRIBUTING.md) — adding IPC commands and platform shells
- [docs/encryption.md](../docs/encryption.md) — Secret Service + keyfile fallback
- [docs/text-expander.md](../docs/text-expander.md) — expander modes on Linux
