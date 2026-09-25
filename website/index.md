<!--# block name="none" --><!--# endblock -->
# Inspector Rust — Clipboard Manager & Command Palette

> Free, open-source (MIT) desktop utility for macOS, Windows and Linux. Ctrl+Space opens a popup with a searchable, encrypted clipboard history, a text expander and 66 search-bar commands: calculator and unit converter, translation, weather, screenshots with OCR and annotation, screen recording, colour picker, system and audio tools. Data stays in a local SQLite database with AES-256-GCM encryption at rest; there is no account and no telemetry.

This is the Markdown version of https://inspector-rust.celox.io/ for agents and text tools. A short summary with every link lives at https://inspector-rust.celox.io/llms.txt.

## Download

- **Newest release:** https://inspector-rust.celox.io/download (picks the file for your platform; always the current release)
- **Current version:** <!--# include virtual="/ssi/version.txt" stub="none" --> · released <!--# include virtual="/ssi/date.txt" stub="none" -->
- **Release data as JSON:** https://inspector-rust.celox.io/latest.json
- **Requirements:** macOS: macOS 11+ · Apple silicon; Windows: Windows 10/11 · x64 · installer; Windows .exe: Windows 10/11 · x64 · no install; Linux .deb: Ubuntu 24.04+ / Debian 13 · x86-64; AppImage: Linux x86-64

Files in the current release:

<!--# include virtual="/ssi/files.md" stub="none" -->

## Features

- **Clipboard history** — Text, images, files and rich text, searchable as you type. Pin what you need often, add a note, and paste with Enter.
- **Text expander** — Type an abbreviation and get the full text — with placeholders for the date, the clipboard or the cursor position. Works in terminals too.
- **66 commands in the search bar** — Calculate, convert units, translate, check the weather, generate passwords or QR codes. Add `?` to any command for its help.
- **Screen tools** — Screenshots with an annotation editor, text recognition (OCR) from any screen region, screen recording to MP4 and a colour picker with a magnifier.
- **System at your fingertips** — Monitor brightness, audio output, disk usage, cache clean-up, live system stats, world clock and calendar — each opens right in the popup.
- **Private by design** — Everything lives in a local database; clipboard contents, snippets and 2FA secrets are encrypted with AES-256-GCM. No account, no telemetry.

### In numbers

Counted from the source code, updated with every change:

- **Lines of code:** <!--# include virtual="/ssi/stat-loc.txt" stub="none" --> (<!--# include virtual="/ssi/stat-loc-detail.txt" stub="none" -->)
- **Unit tests:** <!--# include virtual="/ssi/stat-tests.txt" stub="none" --> (<!--# include virtual="/ssi/stat-tests-detail.txt" stub="none" -->)

### A look inside

- **Clipboard history** — Every copy, searchable as you type — with a preview, notes and pins. ([image](https://inspector-rust.celox.io/assets/gallery/history.jpg))
- **Snippets** — Type a short abbreviation and get the whole text: signatures, addresses, prompts. ([image](https://inspector-rust.celox.io/assets/gallery/snippets.jpg))
- **Calculator and converter** — Maths and unit conversions straight from the search field. ([image](https://inspector-rust.celox.io/assets/gallery/calculator.jpg))
- **Live translation** — `tren`, `trde` and friends translate while you type. Enter copies the result. ([image](https://inspector-rust.celox.io/assets/gallery/translate.jpg))
- **Built-in help** — Type `?` for every command, each with its syntax, examples and tips. ([image](https://inspector-rust.celox.io/assets/gallery/commands.jpg))
- **Weather** — Current conditions, the next twelve hours and five days ahead, for any city. ([image](https://inspector-rust.celox.io/assets/gallery/weather.jpg))
- **System stats** — CPU, memory, battery, temperatures and fans, live or as history. ([image](https://inspector-rust.celox.io/assets/gallery/stats.jpg))
- **2FA codes** — An encrypted authenticator: import from Google Authenticator, Aegis or 2FAS. ([image](https://inspector-rust.celox.io/assets/gallery/totp.jpg))
- **Disk usage** — See what fills your drive as rings, drill in and move files to the Trash. ([image](https://inspector-rust.celox.io/assets/gallery/disk.jpg))
- **World clock** — Your time zones at a glance, day or night. ([image](https://inspector-rust.celox.io/assets/gallery/clock.jpg))
- **Philips Hue** — Switch and dim your lamps and pick colours, from the keyboard. ([image](https://inspector-rust.celox.io/assets/gallery/hue.jpg))
- **QR codes** — Make a code from any text, copy it, or save it as PNG or a printable STL. ([image](https://inspector-rust.celox.io/assets/gallery/qr.jpg))

### Every feature

The complete feature catalogue, mirrored from the repository:

<!--# include virtual="/ssi/features.md" stub="none" -->

## Install

1. **Download for your system** — DMG for Macs with Apple silicon, MSI installer or portable exe for Windows, .deb or AppImage for Linux.
2. **Allow the first launch** — On macOS, move the app to Applications and run `xattr -dr com.apple.quarantine /Applications/InspectorRust.app` once. On Windows, SmartScreen may ask — choose *More info → Run anyway*.
3. **Press Ctrl+Space** — The popup opens wherever you are. On macOS, grant Accessibility once so pasting and the expander can type for you.

## Verify

<!--# include virtual="/ssi/checksums.md" stub="none" -->
['- **Lines of code:** <!--# include virtual="/ssi/stat-loc.txt" stub="none" --> (<!--# include virtual="/ssi/stat-loc-detail.txt" stub="none" -->)', '- **Unit tests:** <!--# include virtual="/ssi/stat-tests.txt" stub="none" --> (<!--# include virtual="/ssi/stat-tests-detail.txt" stub="none" -->)']
- Compare the file with the SHA-256 shown here: `shasum -a 256` on macOS and Linux, `Get-FileHash` in PowerShell.

## FAQ

**Is Inspector Rust free?** Yes. It is free and open source under the MIT licence, with no account, no ads and no telemetry.

**Does it send my clipboard anywhere?** No. The history stays in a local SQLite database, and its contents are encrypted at rest. The app only goes online for commands that need it, such as weather or translation, and only when you run them.

**Which systems does it run on?** macOS 11 or later on Apple silicon, Windows 10 and 11 (x64), and Linux x86-64 (.deb for Ubuntu 24.04+ and Debian 13, or the AppImage). There is no build for Intel Macs, because the on-device background-removal model's runtime has no Intel macOS binary.

**macOS says the app is damaged. What now?** It is not. The release is not notarized, so macOS marks the download as quarantined. Move the app to Applications and run xattr -dr com.apple.quarantine /Applications/InspectorRust.app once in Terminal.

**Can I change the hotkey?** Yes. Ctrl+Space is the default; the popup hotkey, a second clipboard-history hotkey and the feature hotkeys for OCR, screenshots, colour picker and recording can all be changed in the settings.

**How do I back up my data?** Settings has a password-protected export and import of history, snippets, notes, 2FA entries and settings, plus optional scheduled backups into a folder of your choice.

## Limits

- macOS builds are Apple silicon only and not notarized; the first launch needs one Terminal command.
- Some features are macOS-only (for example the network monitor and touchpad gestures).
- Linux: global shortcuts may need to be bound in the desktop settings under Wayland.

## Links

- Source code: https://github.com/pepperonas/inspector-rust
- Changelog: https://inspector-rust.celox.io/changelog.md
- Licence (MIT): https://github.com/pepperonas/inspector-rust/blob/main/LICENSE
- Support the project: https://www.paypal.com/donate/?business=martin.pfeffer@celox.io&currency_code=EUR&item_name=Inspector%20Rust
- Author: Martin Pfeffer, https://celox.io — Imprint https://celox.io/impressum/ · Privacy https://celox.io/datenschutz/
