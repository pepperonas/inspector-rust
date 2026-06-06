# Inspector Rust — macOS bundle

This directory contains the macOS-specific Tauri shell for Inspector Rust. The shared app logic lives in [`../core`](../core); this shell only owns the bundle config (DMG / `.app`), entitlements, and a thin `main.rs` that boots the shared lib.

## Prerequisites (macOS)

- [Rust toolchain](https://rustup.rs/) (Apple Silicon: `aarch64-apple-darwin`; Intel: `x86_64-apple-darwin`) — with `clippy`: `rustup component add clippy`
- [Node.js](https://nodejs.org/) 20+ and [pnpm](https://pnpm.io/) 10+
- **Xcode Command Line Tools** — `xcode-select --install`
- macOS 10.15 (Catalina) or newer

No DMG-specific tooling is required — Tauri's `bundle_dmg.sh` runs out of the box on macOS.

## Build

From the repository root:

```bash
pnpm install                                 # workspace install
pnpm dev:macos                               # tauri dev — live-reload
pnpm build:macos                             # produces .app + .dmg
```

Outputs:

```
target/release/bundle/macos/InspectorRust.app
target/release/bundle/dmg/InspectorRust_x.x.x_<arch>.dmg
```

If only the `.app` is needed (no DMG), build directly:

```bash
cd macos && pnpm tauri build --bundles app
```

## Install

Drag the `.app` from the DMG to `/Applications`, or copy directly:

```bash
cp -R target/release/bundle/macos/InspectorRust.app /Applications/
xattr -dr com.apple.quarantine /Applications/InspectorRust.app   # unsigned dev build
open /Applications/InspectorRust.app
```

The recommended path for developers who rebuild frequently is the `install-macos.sh` script described below — it handles signing, installation, and permission setup in one step.

### Gatekeeper (unsigned builds)

Local builds are not Apple-signed. On first launch macOS will refuse to open the app:

- **Right-click** → **Open** → confirm **Open** in the dialog, **or**
- **System Settings → Privacy & Security → "Open Anyway"** at the bottom.

The `xattr -dr com.apple.quarantine …` command above sidesteps this for development builds.

---

## macOS permissions

Inspector Rust uses four macOS TCC (Transparency, Consent, and Control) surfaces. The table below shows what is required, what it enables, and when macOS prompts for it.

| Permission | System Settings path | Status | Enables |
|------------|---------------------|--------|---------|
| **Accessibility** | Privacy & Security → Accessibility | **REQUIRED** | Paste (`enigo` synthesizes `Cmd+V`), system-wide text expander (abbreviation hotkey + direct snippet slots), input lock (`freeze` command) |
| **Screen Recording** | Privacy & Security → Screen Recording | Required for OCR + screenshot | Screen-region OCR `Ctrl+Shift+O`, screen-region screenshot `Ctrl+Shift+S` |
| **Automation → Finder** | Privacy & Security → Automation | Required for Finder features | Finder selection `Ctrl+Shift+F`, Markdown→PDF `Ctrl+Shift+M` (both read the Finder selection or front window via Apple Events) |
| **Microphone** | Privacy & Security → Microphone | Optional | BPM detector (type `bpm` in the popup) |

### What each permission does

**Accessibility (required)**

Auto-paste uses synthesized `Cmd+V` keystrokes via [`enigo`](https://docs.rs/enigo). The system-wide text expander reads and replaces text in the focused field via macOS Accessibility APIs (`AXValue`, `AXSelectedTextRange`). The `freeze` command installs a `CGEventTap` via `ApplicationServices` + `CoreFoundation` to block keyboard and mouse input until the unlock chord is pressed. All three require Accessibility.

Without Accessibility the popup still opens and entries are readable, but `Enter` will not paste into the previous app, the abbreviation expander hotkey will silently fail (with an amber banner in Settings), and `freeze` will not work.

macOS caches the `AXIsProcessTrusted()` result per process. After granting Accessibility you must **quit and relaunch** Inspector Rust once. The Settings tab detects the newly-granted state and offers a one-click relaunch button.

**Screen Recording**

OCR (`Ctrl+Shift+O`) and screenshot (`Ctrl+Shift+S`) both call `screencapture -i`, which macOS attributes to Inspector Rust and denies if Screen Recording is not granted. The permission is pre-checked via `CGPreflightScreenCaptureAccess` before any capture attempt; a missing grant opens the popup and shows an amber banner pointing at the right Privacy pane.

Note: the eyedropper (`Ctrl+Shift+C`) uses `NSColorSampler` — it does **not** require Screen Recording.

**Automation → Finder**

The Finder selection hotkey (`Ctrl+Shift+F`) and the Markdown→PDF hotkey (`Ctrl+Shift+M`) send Apple Events to `com.apple.Finder` to read the currently selected files or the front window. The first Apple Event triggers the Automation → Finder prompt. This permission covers both features.

**Microphone (optional)**

The BPM detector (type `bpm` in the popup, then press Enter to start) uses the system microphone to detect beats in ambient audio. No audio is recorded or stored — it is processed in real time in the frontend. The microphone prompt fires on first activation.

### Granting permissions: the recommended workflow

Inspector Rust ships a guided helper script that covers all four permissions in a single pass:

```bash
bash scripts/grant-permissions-macos.sh
```

The script:

1. Resets any stale TCC entries for `io.celox.inspector-rust` (so re-granting after a signature change starts clean).
2. Relaunches the app so its own prompts can fire.
3. Instructs you to press `Ctrl+Shift+F` once in Finder to trigger the Automation → Finder prompt.
4. Opens each relevant System Settings → Privacy pane via deep links.
5. Prints a checklist of exactly what to toggle.

**Important:** this script is a guided helper, not an auto-granter. While System Integrity Protection (SIP) is enabled — the default on every Mac — macOS forbids any app or script, including ones running as root, from writing to the TCC database. Only the user can flip the toggles in System Settings. The script makes the one-time setup fast; you still click the switches yourself.

To skip the TCC reset and just open the panes:

```bash
bash scripts/grant-permissions-macos.sh --no-reset
```

### Why grants survive every rebuild — the stable self-signed certificate

Without an Apple Developer ID, macOS TCC keys a permission grant to the app's **(bundle id, cdhash)** tuple. The cdhash is derived from the binary content — so any source change produces a new cdhash and invalidates every existing grant. This meant a `rebuild → grant → rebuild` cycle re-prompted Accessibility every time.

`scripts/install-macos.sh` solves this permanently with a **stable self-signed certificate**:

1. On first run, the script creates a self-signed code-signing certificate in a dedicated local keychain (`~/Library/Keychains/inspector-rust-signing.keychain-db`) with a fixed common name (`Inspector Rust Local Code Signing`).
2. Every build is re-signed with this certificate. With a real certificate — even self-signed — TCC keys the grant to the app's **Designated Requirement**:

   ```
   identifier "io.celox.inspector-rust" and certificate leaf = H"<stable cert hash>"
   ```

   This requirement does **not** reference the cdhash. It stays constant across rebuilds as long as the certificate is unchanged.

3. Net result: **you grant each permission once, and it survives every future rebuild.** The only time a re-grant is needed is after a signature change (e.g., switching from a plain `pnpm build:macos` ad-hoc install to `install-macos.sh` for the first time).

If certificate creation fails for any reason, the script falls back to ad-hoc signing (the previous behaviour).

```bash
# Build + sign + install into /Applications + launch (idempotent)
bash scripts/install-macos.sh

# Same, but also wipe stale TCC entries first — useful when the toggle
# reads "on" in System Settings but a feature still acts denied
bash scripts/install-macos.sh --reset
```

The script prints the cdhash at the end so you can confirm whether the installed binary matches the TCC grant.

The permanent, production-grade fix is an Apple Developer ID (~$99/year). With a Developer-ID signature, TCC matches on (Team ID + bundle id) — independent of cdhash entirely.

### After the first grant: the in-app flow

Once Accessibility is granted, the Settings tab shows a green banner with a **"Restart now"** button. Click it; the app relaunches with the new grant active. For other permissions (Screen Recording, Automation, Microphone) the app detects the grant within ~1 second via background polling — no manual restart needed.

The Settings tab polls each grant once per second while the popup is open, so the status badge flips green roughly 1 second after you toggle the System Settings switch.

### Troubleshooting permissions

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `Enter` does not paste | Accessibility not granted, or granted to an old build | Run `bash scripts/install-macos.sh --reset`, re-grant Accessibility, restart |
| OCR/screenshot returns "permission denied" | Screen Recording not granted | System Settings → Privacy & Security → Screen Recording → enable Inspector Rust |
| `Ctrl+Shift+F` does nothing | Automation → Finder not granted | Press `Ctrl+Shift+F` in Finder once; click Allow in the prompt that appears |
| Abbreviation expander shows amber banner | Accessibility not granted or stale | See Accessibility row above |
| BPM detector shows no level meter | Microphone not granted | System Settings → Privacy & Security → Microphone → enable Inspector Rust |
| Toggle reads "on" but feature still denied | Stale cdhash binding from old ad-hoc build | Run `bash scripts/grant-permissions-macos.sh` to reset and re-grant |

---

## Usage

| Action | Keys |
|--------|------|
| Open popup | `Ctrl+Space` |
| Screen-region OCR (drag a marquee) | `Ctrl+Shift+O` |
| Screen-region screenshot (no OCR) | `Ctrl+Shift+S` |
| Color eyedropper (hex → clipboard) | `Ctrl+Shift+C` |
| Finder selection → batch actions | `Ctrl+Shift+F` |
| Markdown → PDF (selected .md files in Finder) | `Ctrl+Shift+M` |
| Expand snippet abbreviation in place | `Alt+1` (default, configurable) |
| Direct-slot hotkeys (paste snippet body) | Configurable in Settings — work even in terminals |
| Navigate list | `↑` / `↓` |
| Paste selected | `Enter` (or double-click) |
| Close popup | `Esc` (or click outside) |

Inspector Rust runs as a **menu-bar background app** — there is no Dock icon. The activation policy is set to `Accessory` on launch (see [`core/rust-lib/src/lib.rs`](../core/rust-lib/src/lib.rs)).

### Tray menu

- **Open (Ctrl+Space)** — show the popup
- **Manage Snippets** — open the popup directly on the Snippets tab
- **Manage Notes** — open the popup directly on the Notes tab
- **OCR Region (⌃⇧O)** — drag a marquee → recognised text on the clipboard (literal Control, not Cmd — v0.14.1+)
- **Screenshot Region (⌃⇧S)** — drag a marquee → PNG on the clipboard + history, no OCR step (v0.15.0+)
- **Pick Color (⌃⇧C)** — click a pixel anywhere on screen → hex `#RRGGBB` on the clipboard + history (v0.17.0+)
- **Pause Capture** — stop recording new clipboard items
- **Start at Login** — toggle macOS LaunchAgent registration (`~/Library/LaunchAgents/InspectorRust.plist`); checkmark reflects current state (v0.14.0)
- **Clear History…** — wipe all stored entries
- **Quit Inspector Rust**

## Data location

```
~/Library/Application Support/InspectorRust/history.db
```

SQLite database holding clipboard history (capped at 1 000, deduped on SHA-256), snippets, notes, and settings. All sensitive content (clipboard bodies, snippet bodies, note bodies) is AES-256-GCM encrypted at rest; the key lives in the macOS Keychain.

## Files in this directory

```
macos/
├── package.json              # Tauri CLI entry + frontend proxy scripts
├── README.md                 # (this file)
└── src-tauri/
    ├── Cargo.toml            # bin crate; pulls in `tauri/macos-private-api`
    ├── build.rs              # tauri-build
    ├── tauri.conf.json       # bundle = ["dmg","app"], frontendDist = ../../core/frontend/dist
    ├── entitlements.plist    # macOS sandbox/entitlement declaration
    ├── capabilities/         # default + desktop capability permissions
    └── src/
        └── main.rs           # thin entrypoint: inspector_rust_core::run(generate_context!())
```

## Multi-monitor placement

The popup opens on the monitor that contains the mouse cursor at hotkey time. The window is horizontally centered and placed roughly ⅓ from the top of the active monitor; placement is clamped to the monitor's bounds so the popup can never extend past a screen edge — important for mixed-DPI setups (e.g., MacBook Retina + external display). See `core/rust-lib/src/hotkey.rs::clamp_into_monitor`.

## Troubleshooting

- **App opens, hotkey does nothing.** macOS itself binds `Ctrl+Space` to "Select the previous input source" by default — free it up under System Settings → Keyboard → Keyboard Shortcuts → Input Sources, or another app (launcher, IDE) may hold it. Alternatively pick a different popup hotkey in Inspector Rust → Settings, then relaunch.
- **Popup opens but `Enter` does not paste.** Accessibility permission is missing or stale — see "macOS permissions" above. After granting, quit and relaunch Inspector Rust (or click the one-click relaunch button in Settings).
- **`failed to bundle project … bundle_dmg.sh` during `pnpm build:macos`.** The DMG step occasionally fails on busy disks (FileVault background indexing, Time Machine snapshot, etc.). The `.app` itself is already built — install it directly with `cp -R target/release/bundle/macos/InspectorRust.app /Applications/`. Or rebuild only the `.app` with `pnpm tauri build --bundles app`.
- **Tray icon missing after launch.** macOS sometimes hides menu-bar icons when there is no room. Click and drag in the menu bar with `Cmd` held, or use [Bartender](https://www.macbartender.com/) / [Hidden Bar](https://github.com/dwarvesf/hidden) to pin it.
- **OCR or screenshot fails silently.** Screen Recording permission is missing. System Settings → Privacy & Security → Screen Recording → enable Inspector Rust.
- **`Ctrl+Shift+F` or `Ctrl+Shift+M` shows a Finder error.** The Automation → Finder permission has not been granted. Press `Ctrl+Shift+F` once in Finder and click Allow in the system prompt.
