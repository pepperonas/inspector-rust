<div align="right">

[🇬🇧 English](./README.md) · **🇩🇪 Deutsch**

</div>

<div align="center">
  <img src="docs/ir.png?v=6" alt="Inspector Rust — keyboard-first Clipboard-Toolkit" width="600" />

  # Inspector Rust 🕵️‍♂️

  > **Keyboard-first Clipboard-Hyper-Toolkit — nativ auf macOS, Windows 11, Linux. Kein Electron, keine Cloud, keine Telemetrie.**

  Drück **`Ctrl+Space`** irgendwo → rahmenloses Popup über dem aktiven Monitor → Suche durch 1 000 deduplizierte Clipboard-Einträge → Enter pastet zurück in die zuvor fokussierte App. Ganzer Loop unter 200 ms, unter 50 MB RAM, AES-256-GCM-verschlüsselt at-rest mit Keys im OS-Keychain. **Gebaut für die Art Mensch, die schon Muskelgedächtnis für drei Clipboard-Manager hat und von allen genervt ist.**

  ### ✨ Was es kann (in kurz)

  *Grob sortiert nach Alltagsnutzen × Engineering-Aufwand dahinter — Flagship-Features zuerst, Easter-Eggs zuletzt.*

  - 📋 **Clipboard-History** — Text, RTF, HTML, PNG, Datei-Listen; 1 000 Einträge per SHA-256 dedupliziert; **Substring-Suche** while-you-type; jeden Clip pinnen + mit Notiz versehen.
  - 🎯 **Text-Expander — 4 Modi**: passive **Auto-Expansion** (aText-Stil — expandiert beim Tippen, kein Hotkey) · In-Popup-Suche · systemweiter Hotkey (AX/UIA In-Place-Replace + Electron-Fallback) · Direkt-Hotkey → Snippet-Slots (geht auch in Terminals). **Dynamische Platzhalter** zur Paste-Zeit: `{date}` / `{date:%d.%m.%Y}`, `{time}`, `{datetime}`, `{clipboard}`, `{cursor}`, `{{`/`}}`.
  - 🧮 **Inline-Rechner** (`2+2`, `sqrt(144)`, Hex/Bit-Ops; Slot-Machine-Reveal), **Einheiten- / Basis- / Zeit-Konverter** (`5 km in mi`, `0xff in dec`, `1700000000 as date`) und **Farb-Konverter** (`#hex` / `rgb()` / `hsl()` in jede Richtung).
  - 🎚️ **Systemweites Audio-EQ — `boom`** (macOS · Windows via [Equalizer APO](https://sourceforge.net/projects/equalizerapo/)) — ein **10-Band-Grafik-Equalizer + Volume-Boost + 20 Presets** auf den *gesamten* System-Sound, plus **5 Enhancement-Effekte** (Bass · Clarity · Fidelity · Ambience-Stereo-Verbreiterung · Night-Kompressor fürs leise Hören), mit Live-Input/Output-Pegelmetern und **wahrnehmungsgerechter Lautstärke-Kurve** (die Standard-Kurve des virtuellen Treibers machte alles unter 40 % fast unhörbar; boom wendet jetzt einen echten Power-Taper an — der Regler fühlt sich wie echte Hardware an). Installiert einen kleinen virtuellen Audio-Treiber direkt aus dem Panel (ein Klick), matcht die Sample-Rate deines Geräts und **folgt deinem Ausgabegerät live** (inkl. Bluetooth).
  - 🪟 **Fenster-Management** (macOS, opt-in) — zieh ein Fenster an einen Bildschirmrand zum **Snappen** (linke/rechte Hälfte · oben = maximieren, Magnet-Stil), oder schweb über den grünen Zoom-Button für eine **Moom-artige Palette**: Preset-Layouts (⌥ für Viertel) + ein **Honigwaben-Raster** (16×10 Standard, bis 24 — abgerundete Hexagons mit magnetischem Hover, leuchtender Auswahl und Live-Maßanzeige), über das du ziehst, um das Fenster in jede Bildschirmregion zu legen — mit Live-Umriss-Vorschau auf dem Screen.
  - 📸 **Screenshots — CleanShot-X-Stil**: Region (`Ctrl+Shift+S`) · Vollbild · aktives Fenster · Selbstauslöser · Wiederholen; schwebendes Vorschau-HUD; **Annotations-Editor** (Pfeil / Linie / Text / Rechteck / Ellipse / Highlight / Blur / Schwärzen / nummerierte Schritt-Badges); **an Bildschirm pinnen**. Dateinamen enthalten die Quell-App.
  - 🎥 **Bildschirmaufnahme** (`Ctrl+Shift+Alt+S`) — Region ziehen → Audio wählen (System / Mikro / beide) → 3-2-1 → **MP4 (H.264)** nach Downloads; schwebende Leiste mit **Pause/Resume**; Multi-Monitor; System-Audio routet automatisch über ein Loopback. Braucht ffmpeg.
  - 🔍 **Bildschirm-OCR** (`Ctrl+Shift+O`) — Apple Vision (macOS) / WinRT (Windows) / Tesseract (Linux). PDF-Qualität-Texterkennung ins Clipboard.
  - 🎬 **Medien-Tools** — **Download** von YouTube / Instagram / TikTok / Facebook (Video oder Audio — einfach eine URL einfügen; Tab toggelt bei YouTube); **Audio-Swap** (`Ctrl+Shift+Alt+M`) ersetzt oder mischt den Ton eines Videos mit einer lokalen Datei oder einem YouTube-Track; **Trim** von Audio/Video (`trim`) verlustfrei-schnell oder frame-genau. Brauchen ffmpeg / yt-dlp.
  - ⏱️ **Zeiterfassung / Timesheet** (`track on/off`; `track` oder **`Ctrl+Shift+T`**; macOS) — opt-in, event-basierte App-Nutzungserfassung per Fensterfokus mit rückwirkender Idle-Auto-Pause; ein editierbarer **Timesheet-Tab** mit Tages-/Wochen-Ansicht, Inline-SVG-Charts (Timeline · App-Donut · Kategorien · Projekte), **manueller Pause/Weiter-Taste**, CSV- + eigenständigem HTML-Export im sichtbaren Umfang (Tag oder Mo–So-Woche), **wochenweitem Aufräumen** und globalem **Tracking-Hotkey** (`Ctrl+Shift+Alt+T`, umbelegbar); erkennt **Claude-Code**-Nutzung pro Projekt (Zeit + Tokens); optionale **Browser-Extension** (nur Loopback-Socket). Fenstertitel + URLs at-rest verschlüsselt.
  - 📊 **System-Stats** (`stats`) — Live-Inline-Dashboard: CPU (gesamt + pro Kern), Speicher + Swap, **Akku & Leistungsaufnahme in Watt**, Temperaturen + **Lüfter-RPM** (SMC / hwmon), Disks, Live-Netzwerk-Durchsatz, Uptime. **Live ↔ History**-Umschalter mit Linien-Charts pro Metrik (1 h / 6 h / 24 h / 7 d).
  - ☀️ **Monitor-Helligkeit** (`brightness` / `bri`) — Slider inline in der Vorschau für interne *und* externe Displays (**↑↓** Monitor wählen, **←→** anpassen). Software-(Gamma-)Dimming auf macOS + Windows, Hardware-DDC/CI auf Linux. Auf **EDR-fähigen Macs** (14"/16" MBP XDR, Pro Display XDR) läuft *derselbe* Slider **über 100 %** hinaus und hebt das Display in seinen **Extra-Helligkeits-Bereich (EDR/XDR)** — Vivid-Stil, bis ~7× — via Multiply-Blend-Metal-Overlay; macOS drosselt thermisch automatisch (gleicher Pfad wie HDR-Video, innerhalb der Spezifikation).
  - 💡 **Philips Hue** (`hue`) — steuere deine Lampen inline: Alle-Lampen an/aus + Helligkeit, Helligkeit pro Lampe, 8 Farb-Preset-Swatches auf Farb-Bulbs. Plus eine **Beat-Sync**-Disco, die die Lampen zur Musik vom Mikro pulsen lässt. Lokales LAN-Pairing (Discovery oder IP + Link-Button); keine Cloud.
  - 🖐️ **Touchpad-Gesten** (opt-in) — **3-Finger-Swipe** hoch/runter für Lautstärke, **3-Finger-Tap** zum Stummschalten, plus **Tip-Tap-Tab-Wechsel** (macOS): einen Finger auflegen, mit einem zweiten rechts/links daneben tippen → nächster/voriger Tab — dabei sendet IR automatisch **den passenden Shortcut jeder App** (Ctrl+Tab für Browser/Terminals/Finder, ⌘⌥→/← für VS Code/Cursor, ⇧⌘]/[ für JetBrains/Xcode — layoutbewusst aufgelöst, z. B. ⌥6 auf Deutsch). Die Per-App-Zuordnung ist eine mitgelieferte Daten-Datei + User-Override-JSON (`tab-shortcuts.json` im App-Datenordner) — jede weitere App ist ein Eintrag, kein Rebuild. macOS via die private MultitouchSupport-API (schluckt den Swipe, damit das Fenster darunter nicht scrollt); Windows Precision Touchpad; Linux libinput.
  - 🔐 **2FA / TOTP-Manager** — tippe `2fa` *oder* `otp` für den TOTP-Tresor; `otp <issuer>` für sofortige OTP-Autovervollständigung mit Live-30-Sekunden-Countdown. **Hinzufügen / Bearbeiten / Löschen, Drag-Umsortieren und Dedup beim Import**; importiert Google Authenticator / Aegis / 2FAS / **OTPManager (macOS)** / `otpauth` — einfügen *oder* Export-Datei aufs Overlay ziehen. Secrets verschlüsselt, überqueren nie die IPC-Grenze.
  - 🔊 **Audio-Ausgabe** (`sound` / `audio`) — Inline-Picker zum Umschalten des System-Standard-Ausgabegeräts (macOS · Windows · Linux).
  - 🧹 **Aufräumen** (`clean`) — Speicherplatz freigeben durch Löschen von Cache-/Log-/Temp-Dateien in bekannt-sicheren Ordnern. Dry-Run-Vorschau + Bestätigung; strikte Allowlist, Symlinks werden nie verfolgt; Safe / Standard / Aggressive.
  - 🎨 **Farbpipette** (`Ctrl+Shift+C`) — eigene Bildschirm-Lupe mit **Live-Hex unter der Vergrößerung** (macOS) / GDI-Overlay (Windows); Hex direkt ins Clipboard.
  - 🖼️ **Bild-Tools** — Recolor (Logo-Tint), ML-**Freisteller** (U²-Net ONNX, 4,5 MB eingebettet), Lanczos3-**Resize** (`rz`) + **Optimieren** (`optim`, oxipng) auf die Finder-Auswahl oder das Clipboard-Bild.
  - 📁 **Finder-Auswahl-Aktionen** (`Ctrl+Shift+F`, macOS) — Batch-Resize / -Optim / -Freisteller / -Öffnen auf alles, was du im Finder ausgewählt hast.
  - 📄 **Markdown → PDF** (`Ctrl+Shift+M` / `md2pdf`, macOS) — konvertiert die im Finder ausgewählten `.md`-Dateien in-process zu PDF; keine CLI-Tools nötig.
  - 🚀 **App-Launcher** (Spotlight-artig, macOS) — App-Name fuzzy matchen, echtes Icon in der Zeile, Enter startet. Aktiviert eine bereits laufende Instanz statt ein Duplikat zu starten.
  - 🔳 **QR-Code** (`qr <text>`) — Live-Vorschau im Panel; Enter kopiert das PNG ins Clipboard.
  - 🛠️ **Dev-Quick-Tools** — `uuid [n]` · `slug` · `hash` (SHA-256) · `json` (Clipboard pretty-printen) · `jwt` (Clipboard dekodieren) → Clipboard.
  - 🌐 **Web-Such-Bangs** — `g` · `ddg` · `gh` · `yt` · `npm` · `crates` · `so` · `mdn` · `wiki` `<query>` öffnen die Suche der jeweiligen Seite.
  - 🥁 **BPM-Detektor** (`bpm`) — Live-Beat-Erkennung über das Mikro mit animiertem AAA-Visualizer.
  - 💸 **Bruno (Brutto/Netto)** — deutscher Einkommensteuer-Rechner 2025 als Suchleisten-Command. Smarte Defaults + Pro-User-Override in den Einstellungen.
  - ⚙️ **Power-Commands** — die Suchleiste parst Dutzende Shell-artige Commands: Übersetzen (`tr` / `tren` / `trde` / `trde2it` / …), System (`kill` / `lock` / `reboot` / `shutdown` / `mute` / `freeze`), `rnd` / `random` (Würfeln), `timer` / `alarm <HH:MM>`, `touch` / `mkdir` / `terminal` (im offenen Finder-Ordner), `rmvvls`, `pwgen`, `meme [query]` — plus jedes oben genannte Command. Fuzzy-gematcht, immer über den Clips, mit rotem Akzent gerendert.
  - 📓 **Snippets** (27 mitgelieferte KI-Prompts + 255 Material-Farben) · **Notes** (persistente Bookmarks) · **Backup** (Single-File-JSON, optional passwort-verschlüsselt).
  - 🟢 **Keep-alive & Wakelock** — `wakelock on/off` (Alias `caffeine`) hält die Maschine wach (pulsierende Footer-LED + On-Screen-Toast); **„Always keep running"** (Einstellungen → Startup) startet die App nativ neu, falls sie je beendet/gekillt wird.
  - 🔒 **Local-first** — null Netzwerk-Calls, null Account; Daten nur unter `~/Library/Application Support/InspectorRust/history.db`, AES-256-GCM-verschlüsselt mit Keys im OS-Keychain.
  - 🎮 **Versteckte Spiele** — fünf Easter-Egg-Triggerwörter. Du wirst sie finden.

  ### 🧰 Tech-Stack

  Tauri 2 (WebView2 / WKWebView) · Rust-Workspace (`core/rust-lib` geteilt, 2-Zeilen-Per-OS-Bundle-Shells) · React 19 + TypeScript 5 + Tailwind v4 + Vite 7 · Helligkeit via CoreGraphics/GDI-Gamma + DDC/CI (`ddc-hi`). **1437 Unit-Tests (643 Rust + 794 Frontend).** MIT-lizenziert.

  <!-- ── Headline-Kennzahlen — XXL Hero-Badges ─────────────────── -->
  <p>
    <a href="https://github.com/pepperonas/inspector-rust" title="Codezeilen (Rust + TypeScript Quellcode)">
      <img src="https://img.shields.io/badge/lines%20of%20code-~81k-2b3137?style=for-the-badge&logo=rust&logoColor=white" height="64" alt="Lines of code" />
    </a>
    &nbsp;
    <a href="https://github.com/pepperonas/inspector-rust/actions/workflows/ci.yml" title="Unit-Tests — 643 Rust + 794 Frontend, alle grün">
      <img src="https://img.shields.io/badge/unit%20tests-1437%20passing-2ea043?style=for-the-badge&logo=vitest&logoColor=white" height="64" alt="Unit tests" />
    </a>
  </p>

  <!-- ── Highlights — prominente (for-the-badge) Badges ────────── -->
  <p>
    <a href="./LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="MIT License" /></a>
    <a href="https://github.com/pepperonas/inspector-rust/releases/latest"><img src="https://img.shields.io/github/v/release/pepperonas/inspector-rust?style=for-the-badge&label=download&color=1f6feb" alt="Latest release" /></a>
    <a href="#"><img src="https://img.shields.io/badge/platforms-macOS%20%C2%B7%20Windows%20%C2%B7%20Linux-8957e5?style=for-the-badge" alt="Plattformen: macOS, Windows, Linux" /></a>
    <a href="https://tauri.app"><img src="https://img.shields.io/badge/Tauri-2-FFC131?style=for-the-badge&logo=tauri&logoColor=white" alt="Tauri 2" /></a>
    <a href="https://rustup.rs"><img src="https://img.shields.io/badge/Rust-stable-CE422B?style=for-the-badge&logo=rust&logoColor=white" alt="Rust" /></a>
    <a href="#"><img src="https://img.shields.io/badge/privacy-100%25%20local%20%C2%B7%20no%20telemetry-2ea043?style=for-the-badge" alt="Privatsphäre: 100% lokal, keine Telemetrie" /></a>
    <a href="./scripts/check.sh"><img src="https://img.shields.io/badge/clippy-%E2%88%92D%20warnings-CE422B?style=for-the-badge&logo=rust&logoColor=white" alt="clippy -D warnings" /></a>
  </p>

  <!-- ── Status / release ─────────────────────────────────────── -->
  [![CI](https://img.shields.io/github/actions/workflow/status/pepperonas/inspector-rust/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/pepperonas/inspector-rust/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/actions/workflow/status/pepperonas/inspector-rust/release.yml?branch=main&style=flat-square&label=release)](https://github.com/pepperonas/inspector-rust/actions/workflows/release.yml)
  [![Last commit](https://img.shields.io/github/last-commit/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Issues](https://img.shields.io/github/issues/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/issues)
  [![Stars](https://img.shields.io/github/stars/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/stargazers)
  [![Maintenance](https://img.shields.io/badge/maintained-yes-brightgreen?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Unit tests](https://img.shields.io/badge/unit%20tests-1437%20(643%20Rust%20%2B%20794%20TS)-success?style=flat-square)](https://github.com/pepperonas/inspector-rust/actions/workflows/ci.yml)
  [![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=flat-square)](./CONTRIBUTING.md)
  [![Code Style](https://img.shields.io/badge/code%20style-clippy%20%2B%20eslint-orange?style=flat-square)](./scripts/check.sh)
  [![Downloads](https://img.shields.io/github/downloads/pepperonas/inspector-rust/total?style=flat-square&label=downloads&color=8957e5)](https://github.com/pepperonas/inspector-rust/releases)
  [![Code size](https://img.shields.io/github/languages/code-size/pepperonas/inspector-rust?style=flat-square)](#)
  [![Commit activity](https://img.shields.io/github/commit-activity/m/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Top language](https://img.shields.io/github/languages/top/pepperonas/inspector-rust?style=flat-square)](#)

  <!-- ── Platforms ────────────────────────────────────────────── -->
  [![Windows 11](https://img.shields.io/badge/Windows-11-0078D4?style=flat-square&logo=windows11&logoColor=white)](./win)
  [![macOS](https://img.shields.io/badge/macOS-10.15+-000000?style=flat-square&logo=apple&logoColor=white)](./macos)
  [![Apple Silicon](https://img.shields.io/badge/arm64-Apple%20Silicon-555555?style=flat-square&logo=apple&logoColor=white)](./macos)
  [![x86_64](https://img.shields.io/badge/x86__64-supported-555555?style=flat-square)](#)
  [![Linux](https://img.shields.io/badge/Linux-Ubuntu%20%7C%20Debian-brightgreen?style=flat-square&logo=linux&logoColor=white)](./linux/README.md)

  <!-- ── Stack ────────────────────────────────────────────────── -->
  [![React 19](https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=black)](https://react.dev)
  [![TypeScript 5](https://img.shields.io/badge/TypeScript-5-3178C6?style=flat-square&logo=typescript&logoColor=white)](https://www.typescriptlang.org)
  [![Vite 7](https://img.shields.io/badge/Vite-7-646CFF?style=flat-square&logo=vite&logoColor=white)](https://vitejs.dev)
  [![Tailwind CSS v4](https://img.shields.io/badge/Tailwind-v4-38BDF8?style=flat-square&logo=tailwindcss&logoColor=white)](https://tailwindcss.com)
  [![pnpm](https://img.shields.io/badge/pnpm-10-F69220?style=flat-square&logo=pnpm&logoColor=white)](https://pnpm.io)
  [![Node 20](https://img.shields.io/badge/Node-20+-339933?style=flat-square&logo=node.js&logoColor=white)](https://nodejs.org)
  [![SQLite](https://img.shields.io/badge/SQLite-bundled-003B57?style=flat-square&logo=sqlite&logoColor=white)](https://sqlite.org)
  [![ONNX Runtime](https://img.shields.io/badge/ONNX%20Runtime-bundled-005CED?style=flat-square&logo=onnx&logoColor=white)](https://onnxruntime.ai)
  [![Apple Vision](https://img.shields.io/badge/Apple%20Vision-Live%20Text-000000?style=flat-square&logo=apple&logoColor=white)](#)
  [![U2Net](https://img.shields.io/badge/U%C2%B2--Net-cutout-orange?style=flat-square)](https://github.com/xuebinqin/U-2-Net)

  <!-- ── Security & ergonomics ───────────────────────────────── -->
  [![AES-256-GCM](https://img.shields.io/badge/encryption-AES--256--GCM-darkgreen?style=flat-square&logo=letsencrypt&logoColor=white)](./docs/encryption.md)
  [![Keychain-backed](https://img.shields.io/badge/keys-OS%20keychain-555555?style=flat-square)](./docs/encryption.md)
  [![Local-first](https://img.shields.io/badge/local--first-yes-brightgreen?style=flat-square)](#)
  [![No telemetry](https://img.shields.io/badge/telemetry-none-brightgreen?style=flat-square)](#)
  [![Offline](https://img.shields.io/badge/network-not%20required-brightgreen?style=flat-square)](#)
  [![Power user](https://img.shields.io/badge/audience-power%20users-blueviolet?style=flat-square)](#)
  [![Keyboard-first](https://img.shields.io/badge/UX-keyboard--first-blueviolet?style=flat-square)](#)

  <!-- ── Quality ─────────────────────────────────────────────── -->
  [![ESLint](https://img.shields.io/badge/ESLint-flat%20config-4B32C3?style=flat-square&logo=eslint&logoColor=white)](https://eslint.org)
  [![Vitest](https://img.shields.io/badge/Vitest-3-6E9F18?style=flat-square&logo=vitest&logoColor=white)](https://vitest.dev)
  [![cargo test](https://img.shields.io/badge/cargo%20test-594%20passing-success?style=flat-square&logo=rust&logoColor=white)](#)
  [![vitest](https://img.shields.io/badge/vitest-785%20passing-success?style=flat-square&logo=vitest&logoColor=white)](#)
  [![cargo clippy](https://img.shields.io/badge/cargo%20clippy-D%20warnings-success?style=flat-square&logo=rust&logoColor=white)](#)
  [![tsc strict](https://img.shields.io/badge/tsc-strict-3178C6?style=flat-square&logo=typescript&logoColor=white)](#)
  [![Prettier](https://img.shields.io/badge/code%20style-Prettier-F7B93E?style=flat-square&logo=prettier&logoColor=black)](https://prettier.io)

  <!-- ── Community ───────────────────────────────────────────── -->
  [![Issues](https://img.shields.io/github/issues/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/issues)
  [![Closed issues](https://img.shields.io/github/issues-closed/pepperonas/inspector-rust?style=flat-square&color=success)](https://github.com/pepperonas/inspector-rust/issues?q=is%3Aissue+is%3Aclosed)
  [![PRs](https://img.shields.io/github/issues-pr/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/pulls)
  [![Stars](https://img.shields.io/github/stars/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/stargazers)
  [![Forks](https://img.shields.io/github/forks/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/network/members)
  [![Watchers](https://img.shields.io/github/watchers/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/watchers)
  [![Contributors](https://img.shields.io/github/contributors/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/graphs/contributors)
  [![Last commit](https://img.shields.io/github/last-commit/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Commit activity](https://img.shields.io/github/commit-activity/m/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Repo size](https://img.shields.io/github/repo-size/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust)
  [![Code size](https://img.shields.io/github/languages/code-size/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust)
  [![Top language](https://img.shields.io/github/languages/top/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust)
  [![Languages](https://img.shields.io/github/languages/count/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust)
  [![Made with love](https://img.shields.io/badge/made%20with-%E2%99%A5-red?style=flat-square)](#)

  <!-- ── Architecture & build ────────────────────────────────── -->
  [![Monorepo](https://img.shields.io/badge/repo-pnpm%20workspace-F69220?style=flat-square&logo=pnpm&logoColor=white)](./pnpm-workspace.yaml)
  [![Workspace crates](https://img.shields.io/badge/cargo%20workspace-3%20crates-CE422B?style=flat-square&logo=rust&logoColor=white)](./Cargo.toml)
  [![Single binary](https://img.shields.io/badge/distribution-single%20binary-blue?style=flat-square)](#)
  [![Native](https://img.shields.io/badge/no-Electron-success?style=flat-square)](#)
  [![Memory](https://img.shields.io/badge/memory-%3C50%20MB-blue?style=flat-square)](#)
  [![Cold start](https://img.shields.io/badge/cold%20start-%3C200%20ms-blue?style=flat-square)](#)
  [![MSI size](https://img.shields.io/badge/MSI-~5%20MB-blue?style=flat-square&logo=windows&logoColor=white)](#)
  [![DMG size](https://img.shields.io/badge/DMG-~5%20MB-blue?style=flat-square&logo=apple&logoColor=white)](#)
  [![exe size](https://img.shields.io/badge/.exe-~14%20MB-blue?style=flat-square&logo=windows&logoColor=white)](#)

  <!-- ── Features (numerical) ────────────────────────────────── -->
  [![Tests](https://img.shields.io/badge/tests-1437%20passing-success?style=flat-square)](#)
  [![IPC commands](https://img.shields.io/badge/IPC%20commands-197-blueviolet?style=flat-square)](./core/rust-lib/src/commands.rs)
  [![Search-bar commands](https://img.shields.io/badge/search--bar%20commands-65-blueviolet?style=flat-square)](./core/rust-lib/src/commands.rs)
  [![Tauri events](https://img.shields.io/badge/events-26-blueviolet?style=flat-square)](#)
  [![Rust modules](https://img.shields.io/badge/Rust%20modules-57-CE422B?style=flat-square&logo=rust&logoColor=white)](./core/rust-lib/src)
  [![Snippets](https://img.shields.io/badge/AI%20prompts-27%20bundled-blueviolet?style=flat-square)](./docs/ai-prompts.md)
  [![Media](https://img.shields.io/badge/media-record%20·%20download%20·%20trim%20·%20swap-CE422B?style=flat-square)](#)
  [![Motion](https://img.shields.io/badge/motion-Material%203%20Expressive-blueviolet?style=flat-square)](#)
  [![Tabs](https://img.shields.io/badge/popup%20tabs-6-blueviolet?style=flat-square)](#)
  [![DB tables](https://img.shields.io/badge/SQLite%20tables-5-003B57?style=flat-square&logo=sqlite&logoColor=white)](./docs/encryption.md)
  [![Global shortcuts](https://img.shields.io/badge/global%20hotkeys-12-blueviolet?style=flat-square)](#)
  [![Snippet expansion modes](https://img.shields.io/badge/expansion%20modes-4-blueviolet?style=flat-square)](./docs/text-expander.md)
  [![Image formats](https://img.shields.io/badge/image%20formats-5-blueviolet?style=flat-square)](#)
  [![Time tracking](https://img.shields.io/badge/timesheet-event--based%20·%20encrypted-CE422B?style=flat-square)](./docs/timesheet.md)
  [![Privacy](https://img.shields.io/badge/privacy-offline%20·%20no%20telemetry-success?style=flat-square)](./docs/encryption.md)

  <!-- ── Standards / conventions ─────────────────────────────── -->
  [![SemVer](https://img.shields.io/badge/semver-2.0-blue?style=flat-square)](https://semver.org)
  [![Keep a Changelog](https://img.shields.io/badge/changelog-Keep%20a%20Changelog-orange?style=flat-square)](https://keepachangelog.com)
  [![Conventional Commits](https://img.shields.io/badge/commits-conventional-orange?style=flat-square)](https://www.conventionalcommits.org)
  [![ARIA](https://img.shields.io/badge/a11y-keyboard%20first-blueviolet?style=flat-square)](#)
  [![ADRs in CHANGELOG](https://img.shields.io/badge/ADRs-in%20CHANGELOG-orange?style=flat-square)](./CHANGELOG.md)

  <!-- ── Permissions / OS surfaces ───────────────────────────── -->
  [![macOS TCC: Accessibility](https://img.shields.io/badge/macOS%20TCC-Accessibility-000000?style=flat-square&logo=apple&logoColor=white)](./macos/README.md#macos-permissions)
  [![macOS TCC: Screen Recording](https://img.shields.io/badge/macOS%20TCC-Screen%20Recording-000000?style=flat-square&logo=apple&logoColor=white)](./macos/README.md#macos-permissions)
  [![macOS TCC: Automation](https://img.shields.io/badge/macOS%20TCC-Automation%20%E2%86%92%20Finder-000000?style=flat-square&logo=apple&logoColor=white)](./macos/README.md#macos-permissions)
  [![macOS TCC: Microphone](https://img.shields.io/badge/macOS%20TCC-Microphone%20(opt)-000000?style=flat-square&logo=apple&logoColor=white)](./macos/README.md#macos-permissions)
  [![Autostart](https://img.shields.io/badge/autostart-LaunchAgent%20%2F%20RegRun-blue?style=flat-square)](./CHANGELOG.md)
  [![Tray icon](https://img.shields.io/badge/UI-tray%20resident-blue?style=flat-square)](#)

  <!-- ── Tech (extended) ─────────────────────────────────────── -->
  [![rusqlite](https://img.shields.io/badge/rusqlite-bundled-CE422B?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/rusqlite)
  [![enigo](https://img.shields.io/badge/enigo-paste%20sim-CE422B?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/enigo)
  [![clipboard-rs](https://img.shields.io/badge/clipboard--rs-event%20driven-CE422B?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/clipboard-rs)
  [![ort](https://img.shields.io/badge/ort-ONNX%20Runtime-CE422B?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/ort)
  [![ring](https://img.shields.io/badge/ring-AES--256--GCM-CE422B?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/ring)
  [![objc2](https://img.shields.io/badge/objc2-Vision%20FFI-CE422B?style=flat-square&logo=rust&logoColor=white)](https://crates.io/crates/objc2)
  [![lucide-react](https://img.shields.io/badge/icons-lucide--react-F56565?style=flat-square)](https://lucide.dev)
  [![react-virtual](https://img.shields.io/badge/list-react--virtual-FF4154?style=flat-square&logo=react&logoColor=white)](https://tanstack.com/virtual)

  <!-- ── Vibes ───────────────────────────────────────────────── -->
  [![Inspired by Alfred](https://img.shields.io/badge/inspired%20by-Alfred-blueviolet?style=flat-square)](#)
  [![Mouse-free](https://img.shields.io/badge/mouse-not%20required-brightgreen?style=flat-square)](#)
  [![Self-hosted](https://img.shields.io/badge/data-on%20your%20disk-brightgreen?style=flat-square)](#)
  [![Free forever](https://img.shields.io/badge/free-forever-brightgreen?style=flat-square)](./LICENSE)
  [![Made in Germany](https://img.shields.io/badge/made%20in-Germany-FFCE00?style=flat-square)](#)

  Drück `Ctrl+Space` → suchen → einfügen. Inspiriert von Alfreds Clipboard-Viewer auf macOS, eingedampft auf ein Tool, das du auf jeder Maschine behalten kannst.
</div>

---

## Download

**Aktueller Release:** [![Latest Release](https://img.shields.io/github/v/release/pepperonas/inspector-rust?style=flat-square&label=latest&color=green)](https://github.com/pepperonas/inspector-rust/releases/latest) — siehe [CHANGELOG](./CHANGELOG.md) für die Neuerungen.

| Plattform | Datei | Hinweise |
|-----------|-------|----------|
| **Windows 11 / 10** | [`InspectorRust_<ver>_x64_en-US.msi`](https://github.com/pepperonas/inspector-rust/releases/latest) | MSI-Installer — fügt Startmenü-Eintrag & Uninstaller hinzu |
| **Windows 11 / 10** | [`inspector-rust.exe`](https://github.com/pepperonas/inspector-rust/releases/latest) | Standalone-Exe — keine Installation nötig |
| **macOS 10.15+ (Apple Silicon)** | [`InspectorRust_<ver>_aarch64.dmg`](https://github.com/pepperonas/inspector-rust/releases/latest) | DMG für arm64-Macs |
| **macOS Intel** | — | Aus Source bauen: [`macos/README.md`](./macos/README.md) |
| **Linux (Ubuntu/Debian)** | Aus Source bauen — siehe [`linux/README.md`](./linux/README.md) | `.deb` + AppImage via `pnpm build:linux` |

> **macOS-Gatekeeper-Hinweis.** Local-Build-Releases sind **nicht Apple-signiert**. Beim ersten Start weigert sich macOS, die App zu öffnen — Rechtsklick → **Öffnen** → bestätigen, oder **Systemeinstellungen → Datenschutz & Sicherheit → "Trotzdem öffnen"**. Danach die TCC-Berechtigungen erteilen:
>
> | Berechtigung | Benötigt für |
> |--------------|-------------|
> | **Bedienungshilfen** | Paste (`enigo` synthetisiert `Cmd+V`), system-weiter Text-Expander, `freeze`-Eingabesperre |
> | **Bildschirmaufnahme** | OCR (`Ctrl+Shift+O`) und Screenshot-Region (`Ctrl+Shift+S`) |
> | **Automation → Finder** | Finder-Selektion (`Ctrl+Shift+F`) und Markdown→PDF (`Ctrl+Shift+M`) |
> | Mikrofon *(optional)* | BPM-Detektor (Befehl `bpm`) |
>
> Der Settings-Tab zeigt fehlende Grants als zusammenklappbare amber Banner mit Ein-Klick-Sprung zum richtigen Privacy-Pane. `scripts/install-macos.sh` signiert jeden Build mit einem **stabilen selbstsignierten Zertifikat**, sodass alle Grants künftige Rebuilds überleben — jede Berechtigung wird nur einmal erteilt. `scripts/grant-permissions-macos.sh` führt durch das einmalige Setup in einem geführten Durchlauf.
>
> Vollständige Details in [`macos/README.md`](./macos/README.md#macos-permissions).

---

## Plattform-Support

| Plattform  | Status              | Verzeichnis           |
|------------|---------------------|-----------------------|
| Windows 11 | ✅ implementiert     | [`win/`](./win)         |
| macOS      | ✅ implementiert     | [`macos/`](./macos)     |
| Linux      | ✅ implementiert     | [`linux/`](./linux)     |

Die gesamte App-Logik lebt in [`core/`](./core) — ein einzelnes Frontend (`core/frontend`) und eine einzelne Rust-Library (`core/rust-lib`), die plattformübergreifend geteilt werden. Jedes OS hat seine eigene dünne Bundle-Shell mit plattformspezifischen Details (Installer-Config, Icons, Capabilities). Um eine neue Plattform hinzuzufügen, siehe [`CONTRIBUTING.md`](./CONTRIBUTING.md#adding-a-new-platform-shell-linux-etc).

## Workflow

Inspector Rust ist für *einen* Workflow gebaut: **`Ctrl+Space` → tippen → Enter**. Der Hotkey öffnet ein rahmenloses Popup über dem aktiven Monitor; was du tippst, wird fuzzy durchsucht über Clipboard-History, Snippets, Calc-Ergebnisse und Farbwerte; Enter fügt den Top-Match in die zuvor fokussierte App ein. Keine Maus, keine Menü-Bäume, keine Per-App-Integrationen.

Drei weitere globale Shortcuts feuern von überall — Inspector Rusts Fenster muss nicht offen oder fokussiert sein:

- **`Ctrl+Shift+O`** — Bildschirm-Region-**OCR**. Marquee ziehen, Apple Vision erkennt den Text in der Region, der Text landet auf deiner Zwischenablage + oben in der History.
- **`Ctrl+Shift+S`** *(v0.15.0+)* — Bildschirm-Region-**Screenshot**. Gleiche Marquee, kein OCR-Schritt: das aufgenommene PNG geht direkt auf die Zwischenablage und in die History. Ideal für Diagramme, Buttons, Fotos oder Regionen ohne erkennbaren Text. **Als Datei speichern:** Während das Overlay offen ist **`S`** drücken — der Auswahlrahmen wird grün und nach dem Zeichnen erscheint ein nativer Speichern-Dialog statt des Clipboard-Schreibens *(v0.19.2+)*.
- **`Ctrl+Shift+C`** *(v0.17.0+)* — **Eyedropper**. Cursor wird zur NSColorSampler-Lupe (macOS) / GDI-Overlay (Windows); ein Klick auf ein Pixel und der Hex-Code (`#RRGGBB`) landet auf der Zwischenablage + History. Kein Popup, kein Modal — fire-and-forget.

Literal Control auf jedem OS — dieselbe Taste auf Windows und macOS. OCR + Screenshot benötigen auf macOS das **Bildschirmaufnahme**-TCC-Grant; auf Windows sind keine extra Berechtigungen nötig.

Alles andere (Snippets-Verwaltung, Notes, Settings, Image-Tools) lebt im selben Popup hinter Tabs oben rechts — es gibt kein separates Fenster zum Alt-Tabben. **Settings → Keyboard shortcuts** trägt das komplette Cheat-Sheet.

## Features & Shortcuts auf einen Blick

### 🔥🔥🔥 Globale Hotkeys — fire and forget, von überall 🔥🔥🔥

| Shortcut | Aktion | Benötigt (macOS) |
|----------|--------|------------------|
| `Ctrl+Space` | Popup auf dem aktiven Monitor öffnen | — |
| `Ctrl+Shift+V` *(v0.83.0+, konfigurierbar)* | Zweiter **Clipboard-History**-Hotkey — öffnet ebenfalls das Popup | — |
| `Ctrl+Shift+O` | Bildschirm-Region-**OCR** → Text auf Clipboard + History | Bildschirmaufnahme |
| `Ctrl+Shift+S` *(v0.15.0+)* | Bildschirm-Region-**Screenshot** → PNG auf Clipboard + History (kein OCR); **`S`** während Overlay → als Datei speichern (grüner Rahmen) *(v0.19.2+)* | Bildschirmaufnahme *(macOS)* |
| `Ctrl+Shift+Alt+S` *(v0.81.0+)* | **Bildschirmaufnahme** → Bereich → Audio (System / Mic / beides) → 3-2-1 → MP4 nach Downloads. Schwebende Stop-Leiste mit Pause/Resume. Multi-Monitor; ffmpeg | Bildschirmaufnahme *(macOS)* |
| `Ctrl+Shift+C` *(v0.17.0+)* | **Eyedropper** → Hex (`#RRGGBB`) auf Clipboard + History | — |
| `Ctrl+Shift+F` *(v0.30.0+)* | **Finder-Selektion** → Popup mit gerade selektierten Dateien + Actions (Resize, Optim, Cut-Out, …) | Automation → Finder |
| `Ctrl+Shift+M` *(v0.46.0+, macOS)* | **Markdown → PDF** — im Finder gewählte `.md`-Dateien in-process zu PDF konvertieren | Automation → Finder |
| `Ctrl+Shift+Alt+M` *(v0.84.22+, macOS)* | **Audio ersetzen / überlagern** — Video im Finder wählen → Overlay zum Ersetzen oder Mischen einer lokalen Audiodatei oder eines yt-dlp-YouTube-Tracks an gewählter Position | Automation → Finder |
| `Ctrl+Shift+T` *(v0.84.85+, macOS)* | **Timesheet** — Zeiterfassungs-Übersicht öffnen (der Timesheet-Tab) | keine |
| `Ctrl+Shift+Alt+T` *(v0.84.204+, macOS)* | **Zeiterfassung umschalten** — Session von überall starten/stoppen (Status-Toast bestätigt) | keine |
| `Alt+1` *(Default, konfigurierbar, opt-in)* | Snippet-Abbreviation in-place expandieren | Bedienungshilfen |
| *(user-konfigurierbar)* | **Direct hotkey → snippet** — bestimmten Snippet-Body pasten | Bedienungshilfen |

Literal Control auf jedem OS. Dieselbe Taste auf Windows und macOS. Der Expander-Hotkey ist opt-in (aus, bis du ihn in Settings → Text expander konfigurierst).

### Popup-Shortcuts — wenn das Popup offen ist

| Shortcut | Aktion |
|----------|--------|
| `↑` `↓` | In der Liste navigieren |
| `Shift+↑` `Shift+↓` *(v0.22.0+)* | System-Lautstärke erhöhen / senken (±6 % pro Druck) |
| `Enter` | Ausgewählten Eintrag pasten (respektiert das Plain-Text-Setting) |
| `Shift+Enter` | Mit Originalformatierung pasten (überschreibt das Plain-Text-Setting einmalig) |
| `Esc` | Popup schließen |
| `⌘B` / `Ctrl+B` | **Hintergrund freistellen** beim ausgewählten Image-Eintrag (ML — U²-Net) |
| `⌘S` / `Ctrl+S` | **Bild in Downloads speichern** (PNG unverändert) |

### Komplette Feature-Matrix

| Feature | Wo triggern | Doku |
|---------|-------------|------|
| Clipboard-History (Text/RTF/HTML/PNG/Files, 1 000 Einträge, dedupliziert) | `Ctrl+Space` → suchen | core |
| Substring-Suche (Clipboard) + Fuzzy-Match (Commands / Apps) | Im Suchfeld tippen | core |
| **Inline-Taschenrechner** | Ausdruck im Suchfeld tippen (`2+2`, `sqrt(9)`, `sin(pi/2)`, `0xff << 4`, …) | core |
| **Farb-Konverter** | `#RRGGBB` / `rgb(…)` / `hsl(…)` im Suchfeld tippen → Swatch + alle Formate | [colors.md](./docs/colors.md) |
| **HSV-Color-Picker-Modal** | History-Tab → *Color Picker*-Button → Hue-Slider + Swatch + Hex/RGB/HSL-Tabs | [colors.md](./docs/colors.md) |
| **Screen-Eyedropper** (Modal) | *Color Picker*-Modal → *Pick from screen* (macOS `NSColorSampler`-Lupe / Windows GDI-Overlay) | [colors.md](./docs/colors.md) |
| **Eyedropper — globaler Hotkey** *(v0.17.0+)* | `Ctrl+Shift+C` oder Tray *Pick Color* → Hex direkt aufs Clipboard, kein Popup | [colors.md](./docs/colors.md) |
| Snippet-Search-as-you-type | Snippet-Abbreviation im Popup-Suchfeld tippen | [text-expander.md](./docs/text-expander.md) |
| Abbreviation-Expander (system-weit) | Abbreviation in irgendein Textfeld tippen → `Alt+1` (Default) | [text-expander.md](./docs/text-expander.md) |
| Direct hotkey → snippet *(v0.13.0+)* | User-bound globaler Hotkey | [text-expander.md](./docs/text-expander.md) |
| 27 gebündelte AI-Prompt-Snippets (`ai*`) | Snippets-Tab; Search / Abbreviation / Direct-Slot | [ai-prompts.md](./docs/ai-prompts.md) |
| Snippets CRUD + JSON-Import | Snippets-Tab → Formular / Import-Button | [snippets-import.md](./docs/snippets-import.md) |
| Notes — kategorisierte persistente Bookmarks | Notes-Tab (Tray: *Manage Notes*) | [notes.md](./docs/notes.md) |
| Clip als Note speichern | Hover über History-Zeile → Bookmark-Icon | [notes.md](./docs/notes.md) |
| **Bildschirm-Region-OCR** *(v0.9.0+; Windows seit v0.19.2)* | `Ctrl+Shift+O` oder Tray *OCR Region* | core |
| **Bildschirm-Region-Screenshot** *(v0.15.0+; Windows seit v0.19.2)* | `Ctrl+Shift+S` oder Tray *Screenshot Region* | core |
| **Screenshot → als Datei speichern** *(v0.19.2+)* | `Ctrl+Shift+S` → **`S`** während Overlay drücken (grüner Rahmen) → nativer Speichern-Dialog | core |
| **Bild-Recolor** (Logo-Tinten, Chromaticity-gated) | Preview-Pane bei Image-Eintrag → Swatch / Hex | core |
| **ML-Hintergrund-Cutout** (U²-Net-ONNX, ~4,5 MB embedded) | Preview-Pane → *Cut out background* oder `⌘B` | core |
| Bild in Downloads speichern | Preview-Pane oder `⌘S` (PNG unverändert) | core |
| Backup — Single-File-JSON-Export/Import (History + Snippets + Notes, per-Sektion ankreuzbar) | Settings → Backup & restore | [backup.md](./docs/backup.md) |
| Plain-Text-only Paste *(Default an, v0.4.0+)* | Settings → Paste (Shift+Enter überschreibt einmal) | core |
| Autostart bei Login *(v0.14.0+)* | Settings → Startup *oder* Tray-Checkmark | core |
| Clipboard-Capture pausieren | Tray → *Pause Capture* | core |
| History löschen (mit Bestätigung) | Tray → *Clear History…* | core |
| **AES-256-GCM at-rest** (alle Bodies) *(v0.6.0+)* | Automatisch; Key im OS-Keychain | [encryption.md](./docs/encryption.md) |
| Per-Monitor-Popup-Placement | Automatisch (öffnet auf Monitor mit Cursor) | core |
| Multi-Tab-UI | Popup oben-rechts Tabs: History · Snippets · Notes · Features · Settings | core |
| Permissions-UX (TCC-Banner + 1-s-Polling + `tccutil reset`-Recovery) | Settings → Permissions-Sektion *(macOS)* | core |
| Keyboard-Shortcuts-Cheat-Sheet | Settings → *Keyboard shortcuts* (OS-adaptive Glyphen) | core |
| About-Dialog | Settings → About | core |
| **Theme — Hell / Dunkel / System** *(v0.20.0+)* | Settings → Appearance | Drei-Wege-Toggle; Hell/Dunkel überschreiben das OS, System folgt ihm |
| **Power-Command — `tren <text>`** *(v0.18.0+)* | Suchfeld | Englisch → Deutsch übersetzen (öffnet Google Translate im Browser) |
| **Power-Command — `trde <text>`** *(v0.18.0+)* | Suchfeld | Deutsch → Englisch übersetzen (Google Translate) |
| **Power-Command — `tr <text>`** *(v0.18.0+)* | Suchfeld | Text → Deutsch übersetzen (auto-detect Quellsprache) |
| **Power-Command — `rz <W>x<H>`** *(v0.18.0+)* | Suchfeld | Clipboard-Bild via Lanczos3 skalieren (z.B. `rz 1200x800`) |
| **Power-Command — `optim`** *(v0.18.0+)* | Suchfeld | Clipboard-PNG optimieren → `~/Downloads/inspector-rust-optim-<ts>.png` (lossless oxipng) |
| **Power-Command — `rmvvls <text>`** *(v0.18.0+)* | Suchfeld | Vokale entfernen (aeiou + AEIOU + ä/ö/ü) → Clipboard |
| **System-Command — `kill [-9] [pattern]`** *(v0.19.0+)* | Suchfeld — Live-Prozess-Picker | Laufende Prozesse filtern, Enter → Bestätigung → SIGTERM (oder SIGKILL mit `-9`) |
| **System-Command — `reboot`** *(v0.19.0+; Linux/Windows v0.84.0)* | Suchfeld | System neu starten — Confirm zuerst, kein sudo (macOS Apple Events · Windows `shutdown /r` · Linux `systemctl reboot`) |
| **System-Command — `shutdown`** *(v0.19.0+; Linux/Windows v0.84.0)* | Suchfeld | System herunterfahren — Confirm zuerst, kein sudo (macOS · Windows `shutdown /s` · Linux `systemctl poweroff`) |
| **System-Command — `lock`** *(v0.19.0+; Linux/Windows v0.84.0)* | Suchfeld | Bildschirm sperren — sofort, kein Confirm (macOS `pmset` · Windows `LockWorkStation` · Linux `loginctl lock-session`) |
| **System-Command — `mute`** *(v0.23.0+; Linux/Windows v0.84.0)* | Suchfeld | System-Stummschaltung an/aus toggeln (macOS · Windows VK-Taste · Linux `wpctl`/`pactl`) |
| **System-Command — `freeze`** *(v0.28.0+)* | Suchfeld | Tastatur + Maus blocken — entsperren mit konfiguriertem Chord (Default `i + r`) — natives CGEventTap, kein rdev |
| **`wakelock on` / `wakelock off`** *(Alias `caffeine on/off`, v0.52.0+)* | Suchfeld | Computer wachhalten — pausiert Sleep + Bildschirmsperre. macOS `caffeinate`; Windows `SetThreadExecutionState` + unsichtbarer F15-Tastendruck; Linux X11 Cursor-Jiggle. Ein zentrierter Status-Toast bestätigt An/Aus |
| **`touch <name>` / `mkdir <name>` / `terminal`** *(v0.53.0+, macOS)* | Suchfeld | Datei / Ordner anlegen oder Terminal (iTerm2 → Terminal.app) öffnen — im Ordner des vordersten Finder-Fensters (oder Desktop). Braucht Automation → Finder |
| **Finder-Selection-Actions** *(v0.30.0+, macOS)* | `Ctrl+Shift+F` | Popup listet die im Finder selektierten Dateien; `rz 1200x800` tippen skaliert alle Bilder (schreibt `<name>-1200x800.<ext>` neben Quelle), `optim` läuft oxipng auf jedes PNG. Enter auf einer Zeile öffnet die Datei |
| **Resize-Preset-Autocomplete** *(v0.31.0+)* | `rz` oder `rz <partial>` tippen | Beschriftete Preset-Zeilen (Full HD, HD, XGA, SVGA, …); Enter führt aus, Tab / → füllt ins Suchfeld vor dem Ausführen |
| **`bruno <€>[m|j]`** *(v0.33.0+)* | Suchfeld — `bruno 60000` (jährlich) oder `bruno 5000m` (monatlich) | Deutscher Brutto→Netto-Rechner (Steuerjahr 2025); Preview-Panel zeigt volle Aufteilung (KV/PV/RV/AV/ESt/Soli/Kirche/Abgabenquote/Grenzsteuersatz); Enter kopiert Netto-Betrag ins Clipboard. Persönliche Defaults (Steuerklasse, Bundesland, Kinder, Kirche, KV-Zusatz) in Settings → Bruno |
| **Screenshot-Vorschau-HUD** *(v0.32.0+)* | Nach `Ctrl+Shift+S` | CleanShot-X-Style schwebende Karte mit X / Pin / Copy / Save / Edit / Cloud Buttons über dem PNG. Pin behält die Vorschau über den nächsten Screenshot |
| **Annotations-Editor** *(v0.32.0+)* | Vorschau-HUD → Stift-Button | Neues Fenster mit 9 Tools: Pfeil / Linie / Text / Rect / Ellipse / Highlight / Blur (Mosaik-Pixelung) / Redact (deckender Block) / nummerierte Step-Badge. 4 Farb-Presets, 2–16 px Stroke, ⌘Z/⌘⇧Z undo/redo, ⌘S speichern, Esc abbrechen. Save backt zu `<App>-<ts>-edited.png` |
| **App-Name in Screenshot-Dateinamen** *(v0.32.0+)* | Automatisch | `osascript`-gefangener Frontmost-App-Name im gespeicherten Dateinamen: `Safari-20260524-153012.png`. Bearbeitete Varianten bekommen `-edited`-Suffix |
| Power-Command-Autocomplete (Fuzzy-Command-Matching) | Teil-Keyword tippen (`tre`, `rm`, `reb`, `bru`, `tim`, `pw`, …) → Vorschlag als `hint`-Zeile | core |
| **Markdown → PDF** *(v0.46.0+, macOS)* | `Ctrl+Shift+M` mit im Finder ausgewählten `.md`-Dateien | Automation → Finder |
| **2FA / TOTP-Manager** *(v0.47.0+)* | `2fa` oder `otp` tippen → Enter öffnet den TOTP-Tresor: Live-Codes + Countdown, **Hinzufügen / Bearbeiten (inkl. Secret) / Löschen, Drag-Umsortieren (⠿-Griff), Duplikate entfernen / Alle löschen**. Import per Einfügen **oder Datei-Drag&Drop** (Google-Auth-Migration · Aegis · 2FAS · OTPManager · otpauth), dedupliziert beim Import. `otp <issuer>` vervollständigt einen Code inline. Secrets AES-verschlüsselt, überqueren nie IPC | core |
| **OTP-Autocomplete** *(v0.47.0+)* | `otp <Aussteller>` tippen → lebendiger 30-Sekunden-Countdown + Enter kopiert aktuellen Code | core |
| **Timer** | `timer 12` (12 min) · `timer 30s` · `timer 2h` tippen → Countdown + visuelle/akustische Benachrichtigung; Status-Toast beim Setzen | core |
| **Alarm** *(v0.55.0+)* | `alarm 3:00` / `alarm 15:15` tippen → löst zur Uhrzeit aus (nächstes Vorkommen) | core |
| **Markdown → PDF Command** *(v0.55.0+)* | `md2pdf` (Dateimanager-Auswahl) oder `md2pdf <pfad>` → wie `Ctrl+Shift+M`. macOS + Windows (Windows: Pfad angeben; Edge headless) | macOS / Windows |
| **Passwort-Generator** | `pwgen` oder `pwgen 16` tippen → Enter kopiert; Alt+Enter = nur alphanumerisch; Dict- + Leet-Modi im Preview-Panel | core |
| **BPM-Detektor** | `bpm` tippen → Enter startet Live-Takterkennung via Mikrofon; **nochmal Enter pinnt** (Klick außerhalb schließt nicht mehr; Visualizer wird rot) | Mikrofon *(macOS)* |
| **Disco — Lampen im Takt** *(v0.84.46+)* | `disco 1` (an) / `disco 0` (aus) / bare `disco` (toggle) → Mikrofon-getriebener Beat-Sync der Hue-Lampen; **läuft weiter, nachdem das Popup schließt** (gleiche Engine wie die Beat-Sync-Sektion im hue-Panel) | Mikrofon + Hue |
| **Features-Tab** | History · Snippets · Notes · **Features** · Settings Tabs; Features-Tab listet alle Shortcuts und Fähigkeiten mit Live-Hotkey-Anzeige | core |
| **Overlay-Größen-Einstellung** | Settings → Appearance → Popup-Größe: Small / Medium / Large | core |
| **Status-Toast** *(v0.51.0+)* | Zentrierter Bildschirm-Toast bestätigt wakelock an/aus (und andere Zustandsänderungen) mit animiertem Ring | core |
| **Bildschirmaufnahme** *(v0.81.0+, macOS)* | `Ctrl+Shift+Alt+S` → Bereich → Audio (System / Mic / beides, Mic +10 dB) → 3-2-1 → MP4 (H.264) nach Downloads. Schwebende Stop-Leiste mit **Pause/Resume**. Multi-Monitor; System-Audio routet automatisch über ein BlackHole-Multi-Output und stellt danach zurück; `adeclick` + 256 k AAC für sauberen Ton. ffmpeg nötig | core |
| **Audio ersetzen / überlagern** *(v0.84.22+, macOS)* | `Ctrl+Shift+Alt+M` — Video im Finder wählen → Overlay zum **Ersetzen** oder **Mischen** einer lokalen Audiodatei oder eines **yt-dlp-YouTube-Tracks** an gewählter Startposition + Trim. Schreibt `-audioswap.mp4` daneben. ffmpeg (+ yt-dlp) nötig | core |
| **Social-Media-Download** *(v0.84.28+)* | **YouTube / Instagram / TikTok / Facebook**-URL einfügen/kopieren → in Suchleiste oder Clip auto-erkannt → Preview bietet **Video laden** (alle) + **Audio laden** (YouTube) → Downloads. Bevorzugt **H.264** (in QuickTime spielbar); bei YouTubes Bot-Schutz erneuter Versuch mit deinen Browser-Cookies (Chrome/Firefox/…). yt-dlp nötig | core |
| **Audio/Video trimmen** *(v0.84.28+)* | `trim` tippen → lokale Datei wählen → Start/Ende setzen → **verlustfrei & schnell** (`-c copy`) oder **frame-genau** (re-encode) → `-trim`-Kopie. ffmpeg nötig | core |
| **Monitor-Helligkeit** *(v0.62.0+)* | `brightness` (Alias `bri`) → Inline-Slider-Overlay pro Monitor (Software-Gamma-Dimming auf macOS/Windows, DDC/CI auf Linux). Auf EDR/XDR-fähigen Macs läuft der Slider **über 100 %** hinaus und treibt den Extra-Helligkeits-Bereich (EDR) via Multiply-Blend-Metal-Overlay (Vivid-Stil; OS-thermal-gedrosselt) | core |
| **Audio-Ausgabegerät** *(v0.80.0+)* | `sound` (Alias `audio`) → Inline-Picker zum Umschalten des Standard-Ausgabegeräts | core |
| **Philips-Hue-Steuerung** *(v0.84.40+)* | `hue` tippen → Inline-Lampensteuerung in der Vorschau: **Alle-Lampen**-Schalter + Helligkeit, plus eine Zeile pro Lampe mit An/Aus, Helligkeit (←→) und **8 Farb-Presets** (1–8) bei Farb-Lampen. Beim ersten Mal wird die Bridge gekoppelt (lokale SSDP-Discovery oder IP → Link-Button drücken → Connect); nur LAN, keine Philips-Cloud. Eine **Beat-Sync-Sektion** hört aufs Mikrofon und pulst die Lampen im Takt (rainbow/pulse/strobe, Round-Robin-Chase) | core |
| **Aufräum-Tool** *(v0.60.0+)* | `clean` (Alias `cleanup`) → scannt eine Allowlist von Cache-/Log-/Temp-Verzeichnissen → bestätigen → löschen; Safe / Standard / Aggressive in Settings | core |
| **Dev-Quick-Tools** *(v0.76.0+)* | `uuid [n]` · `slug <t>` · `hash <t>` · `json` · `jwt` — UUIDs · slugify · SHA-256 · Clipboard-JSON formatieren · Clipboard-JWT dekodieren → Clipboard | core |
| **Web-Such-Bangs** *(v0.76.0+)* | `g` · `ddg` · `gh` · `yt` · `npm` · `crates` · `so` · `mdn` · `wiki` `<query>` → Site-Suche im Browser öffnen | core |
| **QR-Code** *(v0.76.0+)* | `qr <text>` → Live-Vorschau im Panel; Enter kopiert das PNG in die Zwischenablage | core |
| **Inline-Konverter** *(v0.76.0+)* | `5 km in mi` · `72 f to c` · `0xff in dec` · `1717000000 as date` — Einheiten / Zahlenbasis / Epoch→ISO | core |
| **Smart-Preview-Actions** *(v0.76.0+)* | Text-Clip erkennt URLs / E-Mails / Telefonnummern / `lat,lng` → One-Tap Link öffnen · E-Mail · Anrufen · Karte · QR | core |
| **Zweiter Clipboard-Hotkey** *(v0.83.0+)* | Zweiter konfigurierbarer Popup-Hotkey (Default `Ctrl+Shift+V`) | core |
| **Verschlüsselte Backups** *(v0.79.0+)* | Settings → Backup → optionales Passwort (Argon2id + AES-256-GCM) | [backup.md](./docs/backup.md) |
| **Material-3-Expressive-Motion** *(v0.84.18+)* | Feder-Popup-Entrance, Tab-/Command-/Calc-Übergänge, taktiles Button-Press, Modal-/Toast-Federn — respektiert `prefers-reduced-motion` | core |
| **Calculator Slot-Machine-Reveal** *(v0.84.20+)* | Das Calc-Ergebnis lässt die Ziffern rollen und rastet links→rechts ein; Eingabe + Ergebnis-Zeile rot hervorgehoben wie ein Command | core |

## Features

### Clipboard-Core
- **Globaler Hotkey** `Ctrl+Space` öffnet das Popup zentriert auf dem Monitor mit dem Cursor.
- **Erfasst** Text, RTF, HTML, Bilder (PNG, ≤ 5 MB), und Datei-Listen via OS-nativen Clipboard-Events (kein Polling). Image-vor-Files-Priorität auf macOS, sodass Finder-Image-Copies als Bitmaps landen, nicht als Pfade.
- **Suche** rankt Matches während du tippst: Clipboard-History per **Substring**, Power-Commands + App-Launcher per **Fuzzy** (first-char-anchored Subsequence). Virtualisierte Liste, per-Content-Type Preview-Panel.
- **Auto-Paste** — Enter pasted via `enigo`-simuliertem `Ctrl+V` / `Cmd+V` in die zuvor fokussierte App. Shift+Enter überschreibt das Plain-Text-Setting und pasted mit Originalformatierung.
- **SQLite-Store** unter `%APPDATA%\InspectorRust\history.db` / `~/Library/Application Support/InspectorRust/history.db`. SHA-256-dedupliziert, Cap bei 1 000 Einträgen.
- **AES-256-GCM at-rest** seit v0.6.0 — Text-/HTML-/RTF-/Image-Bodies, Snippet-Bodies, Note-Bodies. Schlüssel im OS-Keychain (Keychain / Credential Manager / Secret Service), 0600-Keyfile-Fallback. Volle Referenz: [`docs/encryption.md`](./docs/encryption.md).
- **Time-Chip** (v0.10.3) — der relative Time-Hint auf jeder Zeile (`just now`, `1h ago`) wird zu einem winzigen klickbaren Button: Hover zeigt sowohl `Captured` als auch `Last used` als absolute Timestamps in einem Tooltip; Klick schaltet den Chip selbst zwischen relativer und absoluter Anzeige um.

### Text-Expander (Snippets, v0.2 — system-weit v0.2.7, Hotkey-Überholung v0.12.0, Direct Slots v0.13.0)
- **Expansion im Popup** — tippe eine Abbreviation ins Suchfeld; matching Snippets erscheinen über Clipboard-Einträgen; Enter pasted den Body.
- **Abbreviation-Expander** — tippe die Abbreviation in *irgendein* Textfeld, drücke den konfigurierten Hotkey (Default `Alt+1`, opt-in via Settings; Ein-Klick-Presets `Alt+1` / `Alt+2` / `Alt+3`, oder beliebige Kombination aufnehmen), Inspector Rust ersetzt sie in-place. Drei Pfade: AX/UIA in-place-Ersatz (native Apps — keine Clipboard-Berührung, kein Flicker, verifiziert durch erneutes Lesen des Werts); AX-select-then-paste-over-selection für Electron / Chromium / Mac-Catalyst-Apps, die `AXValue` read-only freigeben (WhatsApp, Slack, Discord, VS Code — v0.12.0); und ein Clipboard+Keystroke-Fallback für alles andere. Der Diagnose-Button in Settings sagt, welcher Pfad benutzt wurde.
  - *Warum `Alt+1` und nicht `Alt+Backquote`?* Der alte Default war auf deutschen ISO-MacBooks unerreichbar (die physische `^`-Taste meldet sich als `IntlBackslash`). Ziffernreihe-Tasten sind layout-stabil überall. Ein un-customised alter Install wird einmal beim Upgrade auf `Alt+1` migriert (überschreibt keinen Wert, den du absichtlich neu gewählt hast).
- **Direct hotkey → snippet slots (v0.13.0)** — binde einen Hotkey direkt an ein Snippet (Settings → *Direct hotkey → snippet*); Drücken pasted den Body am Cursor mit **keiner getippten Abbreviation**. Liest nichts vom fokussierten Feld — schreibt nur den Body auf die Zwischenablage, synthetisiert Paste, stellt die Zwischenablage wieder her — funktioniert daher in **jeder** App, **inklusive Terminals** (iTerm2, Terminal.app, …), wo der Abbreviation-Expander die Input-Zeile nicht sehen kann. Kollisionen mit Popup-/OCR-/Abbreviation-Hotkeys werden abgelehnt.
- **Laut bei Permission-Fail (macOS, v0.12.0)** — wenn Accessibility nicht erteilt ist, no-opt der Hotkey nicht länger still: Inspector Rust öffnet sein Popup, wechselt auf Settings und zeigt ein amber Banner mit `Force re-grant` → `Restart now`. (Selbes Pattern wie OCR-/Paste-Banner. Direct Slots nutzen dasselbe Gate + Banner.)
- **Snippets-Tab** zum Erstellen/Editieren/Löschen mit zweispaltigem Formular. **JSON-Import** via Snippets → Import (`docs/snippets-import.md`, thematische Samples in `docs/examples/snippets/`).
- **Funktioniert überall, inkl. Terminals (v0.64.0)** — ist der Hotkey aktiv, merkt sich ein passiver Tastatur-Tracker die gerade getippte Abkürzung, sodass `Alt+1` sie aus diesem Puffer expandiert (Blind-Backspace + Paste), **ohne** das fokussierte Feld zu lesen. Die AX/UIA-In-Place-Pfade bleiben als Fallback. Image-/Files-Snippets werden nicht expandiert (nur Text).
- Volle Referenz: [`docs/text-expander.md`](./docs/text-expander.md).

### 27 gebündelte AI-Prompt-Snippets (v0.5.0, überarbeitet v0.12.0)
First-Launch seedet deine Snippet-Tabelle mit `ai*`-prefixed Prompts über Programmierung, Web, IT-Security, Business, Daten und API-Design (`aiplan`, `aireview`, `airefactor`, `airegex`, `aisql`, `aitest`, `aimigration`, `aithumb`, `aithreat`, `aipentest`, `aibrief`, `aiml`, `aiapi`, …). Jeder Prompt ist die **strukturierte Anweisungs-Hälfte only** — keine `[REQUIREMENT]`-artigen Fill-in-Slots (entfernt in v0.12.0). Du hängst ihn an deinen eigenen Prompt / Code / Kontext an und das LLM nimmt das Thema von dort auf. Idempotent (gelöschte Prompts bleiben gelöscht), wiederherstellbar von der Snippets-Sidebar — existierende Installs klicken *Restore defaults*, um den v0.12.0-Stil aufzugreifen. Komplette Liste: [`docs/ai-prompts.md`](./docs/ai-prompts.md).

### Inline-Taschenrechner (v0.2.5)
Tippe einen Mathe-Ausdruck ins Suchfeld, das Ergebnis erscheint als oberster Listen-Eintrag — Alfred-Style. Enter zum Pasten.

- Operatoren `+ - * / % ^`, unär `+/-`, Klammern. Zahlen: int/dezimal/wissenschaftlich/`1_000`-gruppiert. Konstanten: `pi`/`π`, `tau`, `e`. Funktionen: `sqrt`, `cbrt`, `abs`, `sign`, `floor`/`ceil`/`round`, `ln`/`log`/`log2`, `exp`, Trig + Hyperbolisch + Invers, `min`/`max`/`pow`/`mod`.
- Aktiviert nur bei Ausdrücken mit mindestens einem Operator/Function/Konstante — pure Zahlen und Text triggern nicht. Force-Evaluation einer Literale mit `=`-Prefix (`=pi`).
- Sicherer Recursive-Descent-Parser in [`calc.ts`](./core/frontend/src/lib/calc.ts), kein `eval`. 27 Tests.

### Farb-Tools (v0.4.0 → v0.5.2)
- **Inline-Hex-Preview** — tippe `#3366FF` (auch `3366ff`, `#abc`, `#abcdef12`) → Swatch + Hex + RGB-Zeile oben → Enter pasted Großbuchstaben `#RRGGBB`.
- **HSV-Picker-Modal** — Hue-Slider, großes Swatch, Output-Tabs für Hex / RGB / HSL, Zwei-Klick-Auswahl (kein stiller Default), Copy via Tauri-Clipboard-Plugin (umgeht WKWebView-Restriktionen).
- **Pixel vom Bildschirm picken** — sample irgendein Pixel auf dem Desktop. macOS: Apples `NSColorSampler`-Lupe. Windows: Fullscreen-Overlay + `GetPixel`. Modul: [`screen_picker.rs`](./core/rust-lib/src/screen_picker.rs).
- Frontend in [`colors.ts`](./core/frontend/src/lib/colors.ts) + [`ColorPickerModal.tsx`](./core/frontend/src/components/ColorPickerModal.tsx). 32 Tests. Referenz: [`docs/colors.md`](./docs/colors.md).

### Bildschirm-Region-OCR (v0.9.0, macOS)
Drück `Ctrl+Shift+O` (oder nutze den Tray-Eintrag **OCR Region**) → Marquee über jeden Text auf dem Bildschirm ziehen → Inspector Rust läuft Apple Vision über die Auswahl und schreibt den erkannten Text direkt auf deine Zwischenablage. Der Text landet oben in der History; das Source-PNG wird als separater Image-Eintrag direkt darunter aufbewahrt, sodass du eine andere Region nochmal OCR'en kannst ohne den Screenshot neu zu machen, und Enter auf dem auto-selected Top-Eintrag pasted den **Text**, nicht den Screenshot (Ordering gefixt in v0.14.2). Der Hotkey ist **literal Control** auch auf macOS (v0.14.1+ — frühere Builds nutzten `⌘⇧O`, was mit IDE-Bindings kollidierte).

- **Region-Picker** — nutzt `screencapture -i` (dasselbe Binary wie Cmd+Shift+4), sodass die Marquee-UX die polierte ist, die User schon kennen. Esc cancelt sauber.
- **Engine** — Visions `VNRecognizeTextRequest` mit accuracy=Accurate + Sprach-Korrektur; selbe Engine, die Apple Live Text antreibt. Kein Model-Bundling, kein Netzwerk.
- **Sprachen** — was auch immer dein macOS-Vision-Install unterstützt (Latein + CJK + Arabisch + Kyrillisch auf macOS 13+).
- **Windows** *(v0.19.2+)* — implementiert via WinRT `Windows.Media.Ocr` + `Windows.Graphics.Imaging`. Nutzt die bereits auf deinem Windows-System installierten Sprachpakete (Einstellungen → Zeit & Sprache → Sprache) — keine Extras nötig.
- Module: [`region_picker.rs`](./core/rust-lib/src/region_picker.rs), [`ocr.rs`](./core/rust-lib/src/ocr.rs).

### Image-Tools — Recolor + ML-Cutout + Save (v0.7.0 → v0.10.x)
Auf ausgewählten Image-Einträgen zeigt das Preview-Panel drei Aktionen:

- **Recolor** (v0.7.0) — für überwiegend graustufige PNGs (Logos / Icons / Silhouetten), 9 Preset-Swatches + Custom-Hex färben das Bild. RGB-Lerp von Target → Weiß pro Pixel-Luminanz, Alpha bleibt erhalten. Gesättigte Fotos werden automatisch aus der Toolbar versteckt (Chromaticity-Gate). Fügt die getintete Version als neuen History-Eintrag hinzu; das Original bleibt.
- **Cut out background** (v0.10.0) — lässt das **U²-Net (U2Netp) ONNX-Model** (~4,5 MB embedded) über das Bild laufen, um das Foreground-Subject zu detektieren; Output ist ein transparentes PNG, gespeichert nach `~/Downloads/<name>-cutout-<ts>.png`. Shortcut `Cmd/Ctrl+B`. Funktioniert mit echten Fotos (Flugzeug am Himmel, Person vor unruhigem Hintergrund, …) — selbe Architektur wie Pythons `rembg`, nur ohne Python. Inference läuft via `ort` (ONNX Runtime, statisch ins Binary gelinkt).
- **Save to Downloads** (v0.10.1) — drop den ausgewählten Image-Eintrag auf die Platte als `~/Downloads/inspector-rust-image-<ts>.png` unverändert. Shortcut `Cmd/Ctrl+S`. Companion zum Recolor: wähle den frisch-getinteten History-Eintrag, drück `Cmd+S`, deine Datei liegt in Downloads.
- **Inputs:** PNG, JPEG, WebP, GIF, BMP — für Clipboard-Image-Einträge *und* Single-File-Files-Einträge (eine aus Finder kopierte JPG funktioniert also auch). Output ist immer RGBA-PNG.
- Module: [`recolor.rs`](./core/rust-lib/src/recolor.rs), [`cutout_ml.rs`](./core/rust-lib/src/cutout_ml.rs). Der Legacy-Chroma-Key-Cutout in [`cutout.rs`](./core/rust-lib/src/cutout.rs) wird als Fast-Path-Option behalten, aber per Default nicht benutzt. 16-MP-Cap auf Inputs. Gebündeltes Model: [`core/rust-lib/models/u2netp.onnx`](./core/rust-lib/models/u2netp.onnx) (Apache-2.0).

### Notes (v0.2.6)
Persistente, kategorisierte Clipboard-Items in einer separaten SQLite-Tabelle — **nicht** unterworfen dem 1 000-Einträge-Pruning.

- **Bookmark aus History** — Hover über jede Zeile → Bookmark-Icon → Eintrag landet in Notes/`Uncategorized`. Entkoppelt vom Source-Clip; überlebt Pruning.
- **Notes-Tab** — drei Panes: Kategorien-Sidebar (mit Counts; virtuelle `All` / `Uncategorized`), Liste, Detail/Edit. Frei-formige Kategorien (`<datalist>`-Autocomplete). Editierbare Bodies für Text/HTML/RTF; Image-/Files-Notes sind read-only. Per-Row-Delete + Clear All mit Bestätigung.
- **+ New Note** für from-Scratch-Einträge. Tray-Shortcut: **Manage Notes** öffnet das Popup direkt hier.
- Referenz: [`docs/notes.md`](./docs/notes.md).

### Backup — Single-File-JSON-Export/Import (v0.2.6+)
Settings-Tab → *Backup & restore* → History / Snippets / Notes einzeln ankreuzen → Export in eine JSON-Datei. Import merged zurück: Snippets upsert nach Abbreviation, History upsert nach SHA-256, Notes appended. Versioniertes Schema — neuere Backups werden abgelehnt statt still zu kappen. Referenz: [`docs/backup.md`](./docs/backup.md).

### Plain-Text-Paste (Default an, v0.4.0)
HTML- / RTF-Clipboard-Einträge werden zur Paste-Zeit auf ihren Text-Preview gestrippt, sodass Copy-aus-Word / -Browser / -Mail nicht länger Styling in andere Apps leakt. Toggle in Settings → Paste. Shift+Enter im Popup überschreibt für einen Paste.

### Permissions-UX (v0.11.0)
Inspector Rust nutzt **vier** unabhängige macOS-TCC-Surfaces. Der Settings-Tab zeigt jede als zusammenklappbares amber Banner:

| Berechtigung | Aktiviert | Banner erscheint wenn fehlend |
|--------------|----------|------------------------------|
| **Bedienungshilfen** | Paste, Text-Expander, `freeze` | Bei jedem Paste-Versuch + Expander-Hotkey |
| **Bildschirmaufnahme** | OCR (`Ctrl+Shift+O`), Screenshot (`Ctrl+Shift+S`) | Wenn OCR oder Screenshot versucht wird |
| **Automation → Finder** | Finder-Selektion (`Ctrl+Shift+F`), Markdown→PDF (`Ctrl+Shift+M`) | Wenn Hotkey ohne Grant gedrückt wird |
| **Mikrofon** | BPM-Detektor (`bpm`) | Wenn BPM-Modus aktiviert wird |

Jedes Banner:
- Bleibt laut (Border + Warn-Icon + primärer `Open System Settings`-Button), wenn fehlend, kollabiert aber per Default zu einer einzelnen Zeile.
- Pre-checked, bevor der relevante native Call invoked wird. OCR returnt eine `screen.permission_denied`-Sentinel statt still zu failen; ein Tauri-Event öffnet das Popup und zeigt das Banner.
- Pollt das Grant einmal pro Sekunde, sodass das Badge ~1 Sekunde nach dem System-Settings-Toggle auf grün flippt — kein Reload nötig.
- Hat einen `tccutil reset`-Recovery-Button für den "Toggle ist an, aber der laufende Prozess sieht immer noch denied"-Stale-cdhash-State.

`scripts/install-macos.sh` signiert jeden Build mit einem stabilen selbstsignierten Zertifikat, sodass Grants Rebuilds überleben. `scripts/grant-permissions-macos.sh` bietet ein geführtes Einmal-Setup für alle vier Berechtigungen. Vollständige Details: [`macos/README.md`](./macos/README.md#macos-permissions).

### Discoverability (v0.10.7)
- **Footer-Hints** — `⌃⇧O OCR` + `⌃⇧S Shot` + `⌃⇧C Color` neben dem `⏎ Paste · ↑↓ Navigate · Esc Close`-Strip gerendert, sodass User alle globalen Shortcuts jedes Mal sehen, wenn sie das Popup öffnen.
- **Settings → Keyboard shortcuts** — Drei-Gruppen-Cheat-Sheet (Global / Popup-Nav / Image-Actions), das jeden Shortcut der App abdeckt. Modifier-Glyphs (`⌘` vs `Ctrl`, `⇧` vs `Shift`, `⌥` vs `Alt`) passen sich ans laufende OS an via dem `IS_MAC`-Helper in [`core/frontend/src/lib/platform.ts`](./core/frontend/src/lib/platform.ts).
- **About-Dialog** — Settings → About öffnet ein Modal mit Version, License, Jahr, Zielgruppe und einer tabellarischen Tech-Stack-Übersicht.

### Screenshot-Vorschau-HUD + Editor (v0.32.0)
- **CleanShot-X-Style-HUD** — nach `Ctrl+Shift+S` schwebt der PNG als Hintergrund einer kleinen dunklen Karte mit sechs Controls darüber: **X** (oben-links, verwerfen), **Pin** (oben-rechts, Vorschau über nächsten Screenshot halten), **Copy** + **Save** (mittlere Pillen), **Stift** (Editor öffnen), **Cloud** (Placeholder — kommt noch).
- **App-Name im Dateinamen** — `osascript` liest die Frontmost-App *bevor* der Region-Picker startet; gespeicherte Datei wird `Safari-20260524-153012.png`. Alphabetische Sortierung im Finder gruppiert nach App. Bearbeitete Varianten bekommen `-edited` Suffix.
- **Annotations-Editor** — Stift öffnet ein separates Tauri-Fenster mit fünf Tools: **Pfeil / Text / Rect / Highlight / Blur** (Mosaik-Pixelung, sampled aus der Quelle, also Undo non-destruktiv). 4 Farb-Presets, 2–16 px Stroke. Hotkeys: `⌘Z`/`⌘⇧Z` undo/redo, `⌘S` speichern, `Esc` abbrechen, Tool-Switches per Einzel-Taste (`A`/`T`/`R`/`H`/`B`). Canvas ist in nativer Pixel-Auflösung des Screenshots, das gespeicherte PNG also full-resolution.
- **Pin-Verhalten** — solange gepinnt, schreibt der nächste Screenshot zwar weiterhin in Clipboard + History, ersetzt aber nicht die sichtbare Vorschau. Nützlich für Batch-Capture-and-Annotate-Workflows.

### Media-Tools — aufnehmen · downloaden · trimmen · tauschen (v0.81.0 → v0.84.x, ffmpeg)
- **Bildschirmaufnahme** (`Ctrl+Shift+Alt+S`, macOS) — Bereich wählen → Audio (System / Mic / beides, Mic +10 dB) → 3-2-1 → MP4 (H.264) nach Downloads, mit schwebender Stop-Leiste die **pausieren/fortsetzen** kann (Segmente + verlustfreies Concat). Multi-Monitor (nimmt den Bildschirm unter dem Cursor auf). System-Audio routet automatisch über ein BlackHole-Multi-Output und stellt deinen Default danach wieder her; der Ton wird ent-klickt (`adeclick`), zeitkorrigiert (`atempo`) und mit 256 k AAC / 48 kHz kodiert. Arg-Builder + Audio-Sync-Mathematik sind pure + unit-getestet.
- **Audio ersetzen / überlagern** (`Ctrl+Shift+Alt+M`, macOS) — Video im Finder wählen → Overlay zum **Ersetzen** der Tonspur oder **Mischen** einer neuen darüber, an gewählter Startposition mit optionalem Trim + Lautstärke pro Spur. Die neue Audio ist eine lokale Datei oder ein **yt-dlp-YouTube-Track**. Video wird stream-kopiert (schnell/verlustfrei), Ausgabe ist ein `-audioswap.mp4` daneben.
- **Social-Media-Download** — **YouTube / Instagram / TikTok / Facebook**-URL einfügen/kopieren; auto-erkannt (in Clip oder Suchleiste), die Preview bietet **Video laden** (alle) + **Audio laden** (YouTube). H.264 wird bevorzugt, damit die Datei in QuickTime spielt; bei YouTubes „confirm you're not a bot"-Sperre wird transparent mit deinen Browser-Cookies (Chrome / Firefox / Brave / Edge) erneut versucht. Dateien landen in `~/Downloads` mit dem **Download-Zeitstempel** (sortieren also neueste-zuerst). Per yt-dlp.
- **Trimmen** (`trim`-Command) — lokale Audio-/Videodatei wählen, Start/Ende auf einer Timeline setzen, und schneiden — **verlustfrei & schnell** (`-c copy`, snapt auf Keyframes) oder **frame-genau** (re-encode). Speichert eine `-trim`-Kopie.

### Meme-Bibliothek (v0.70.0) — `meme [query]` tippen

`meme [query]` durchsucht fuzzy einen Ordner mit GIFs/Bildern, zeigt eine animierte Vorschau und kopiert das gewählte Meme bei Enter ins Clipboard (auf macOS als Datei-URL, damit die Animation beim Einfügen in einen Chat erhalten bleibt). Der Ordner ist **nicht in die App eingebaut** — zeig auf deine eigene Sammlung oder schnapp dir das kuratierte Starter-Pack unten.

**📦 Starter-Pack herunterladen:** **[`inspector-rust-memes.zip`](https://github.com/pepperonas/inspector-rust/releases/latest/download/inspector-rust-memes.zip)** (~126 MB, 351 Reaction-GIFs in 14 Kategorien) — auch im Repo unter [`memes/`](./memes) durchsuchbar.

**Installation (3 Schritte):**
1. **Lade** `inspector-rust-memes.zip` vom [neuesten Release](https://github.com/pepperonas/inspector-rust/releases/latest) (oder kopiere den [`memes/`](./memes)-Ordner aus einem Repo-Clone).
2. **Entpacke** es — es entsteht ein `memes/`-Ordner mit Kategorie-Unterordnern (`feels/`, `deal-with-it/`, …).
3. **Leg es dorthin, wo die App sucht**, entweder:
   - **Standard-Pfad** *(empfohlen — aktiviert die animierte Vorschau)*: verschiebe den Inhalt hierhin:
     - macOS / Linux: `~/My Drive/media/memes`
     - Windows: `%USERPROFILE%\My Drive\media\memes` (oder `G:\My Drive\media\memes`, falls Google Drive im Streaming-Modus läuft)
   - **Beliebiger Pfad**: leg den Ordner irgendwohin und trage ihn unter **Settings → Meme library** ein (oder lass das Feld leer, um auf den Standard zurückzusetzen). Ein eigener Ordner listet + kopiert problemlos; die *animierte* In-App-Vorschau rendert nur innerhalb des Standard-Pfads (Asset-Protocol-Scope).

Dann das Popup öffnen und `meme` tippen (optional `meme katze` zum Filtern). Unterordner-Namen werden zu Kategorien; der Dateiname (ohne Endung) ist das durchsuchbare Label. Unterstützt: `gif · png · jpg · jpeg · webp · bmp · apng`. Das ganze Feature lässt sich mit `pnpm build:{macos,win,linux}:nomeme` herauskompilieren.

### Finder-Selection-Actions (v0.30.0, macOS)
- **`Ctrl+Shift+F`** — `osascript` liest die Finder-Selection (mit TCC-Automation→Finder-Grant, beim ersten Mal angefragt). Popup öffnet sich mit den selektierten Dateien an der Spitze, jede mit `finder`-Chip.
- **Multi-File-`rz`** — `rz 1200x800` im Finder-Mode skaliert jedes selektierte Bild, schreibt `<name>-1200x800.<ext>` neben Quelle (Format bleibt). Originale unangetastet.
- **Multi-File-`optim`** — gleiche Form: oxipng auf jedes selektierte PNG, schreibt `<stem>-optim.png` neben Quelle. Non-PNG-Selektionen werden übersprungen (oxipng-only).
- **Permission via Settings** — die macOS-Permission-Karte hat drei Zeilen (Bedienungshilfen · Bildschirmaufnahme · Automation → Finder); "Set up permissions" cyclet alle drei mit einem Klick via `tccutil reset` + Re-Prompt.

### Bruno — Brutto/Netto-Rechner (v0.33.0)
- **Befehl** — `bruno 60000` (jährlich) oder `bruno 5000m` (monatlich) im Suchfeld. Ergebnis-Zeile zeigt Netto/Monat + Netto/Jahr inline; Preview-Panel zeigt volle Aufteilung (KV / PV / RV / AV + ESt / Soli / Kirche + Abgabenquote + Grenzsteuersatz).
- **Smart Defaults** — Steuerklasse I, NRW, 0 Kinder, kein Kirchen-Mitglied, TK-Niveau 2,45 % KV-Zusatz. Persönliche Werte via **Settings → Bruno** (in SQLite-Settings persistiert; `bruno-defaults-changed`-Event aktualisiert das Popup ohne Restart).
- **Steuerjahr 2025** — §32a EStG (vereinfacht), Grundfreibetrag 12.096 €, Beitragsbemessungsgrenzen KV 66.150 € / RV 96.600 €. Portiert aus der [Steuerschleuder](https://steuerschleuder.celox.io/)-Web-App des Maintainers.
- **Pure-TS-Compute** — kein IPC-Roundtrip pro Tastendruck. Zahlenformat-toleranter Parser (`bruno 60.000` = `bruno 60,000` = `bruno 60000`). 32 Unit-Tests pinnen Compute + Parser. ⚠️ Vereinfacht — kein Faktorverfahren, keine individuellen Freibeträge. Keine Steuerberatung.

### `freeze` (v0.28.0)
- Natives macOS-`CGEventTap` (raw FFI auf `ApplicationServices` + `CoreFoundation`) blockt alle Tastatur- + Maus-Eingaben bis der konfigurierte Unlock-Chord (Default `i + r`) gedrückt wird. Installiert auf dem Main-Run-Loop via `CFRunLoopGetMain()` — Worker-Thread-Varianten haben Events auf Sonoma+ stillschweigend nicht gedroppt.

### `wakelock` / `caffeine` (v0.29.0 · `on`/`off`-Syntax v0.52.0)
- **`wakelock on`** hält den Mac wach, **`wakelock off`** stoppt. **`caffeine on`/`off`** ist ein Alias. (Die alte `wakelock=1`/`=0`-Syntax wurde in v0.52.0 entfernt.) Wachhalten pausiert Sleep + Bildschirmsperre, trickst Teams- / Slack- / Discord-"Abwesend"-Detection und Bildschirmschoner-/Lock-Idle-Timer aus. Mechanismus pro Plattform: macOS `caffeinate -disu` (echte IOPM-Assertions); Windows `SetThreadExecutionState` **plus** unsichtbarer `F15`-Tastendruck alle 30 s (setzt den Screensaver-/Lock-Idle-Timer zurück, nicht nur Power-Sleep); Linux X11 Cursor-Jiggle (Wayland: no-op). Beim Umschalten schließt sich das Popup und ein zentrierter **Status-Toast** bestätigt den neuen Zustand.

### `touch` / `mkdir` / `terminal` (v0.53.0, macOS)
- Mit offenem Finder-Fenster legt **`touch <name>`** eine leere Datei, **`mkdir <name>`** einen Ordner an, oder **`terminal`** öffnet ein Terminal — jeweils **im aktuellen Ordner dieses Fensters** (oder Desktop, wenn kein Fenster offen ist — Finders `insertion location`). `touch`/`mkdir` selektieren das neue Element im Finder; Namen werden bereinigt (kein `/`, `.`, `..`). `terminal` bevorzugt **iTerm2**, sonst Terminal.app. Alle brauchen den Automation→Finder-TCC-Grant (wie Finder-Selection).

### System-Tray + Multi-Monitor
- **Tray-Menü:** Open · Manage Snippets · Manage Notes · **OCR Region (Ctrl+Shift+O)** · **Screenshot Region (Ctrl+Shift+S)** *(v0.15.0+)* · **Pick Color (Ctrl+Shift+C)** *(v0.17.0+)* · Pause Capture · ☑/☐ Start with Windows / Start at Login (Checkmark spiegelt State seit v0.14.0) · Clear History · Quit.
- **Autostart bei Login** (v0.14.0) — Toggle in Settings → Startup oder vom Tray-Menü. macOS schreibt `~/Library/LaunchAgents/InspectorRust.plist`; Windows nutzt den Run-Key-Registry-Eintrag. App startet hidden im Tray, sodass sie bereit ist, wenn der Popup-Hotkey trifft.
- **Multi-Monitor-Placement:** Popup öffnet auf dem Monitor mit dem Cursor, horizontal zentriert, ~⅓ von oben, geclamped auf die Bounds des aktiven Monitors (wichtig bei Mixed-DPI-Setups).

## Repository-Layout

```
inspector-rust/
├── core/
│   ├── frontend/            # React 19 + TS + Tailwind v4 (plattformübergreifend)
│   │   └── src/
│   │       ├── components/  # SearchBar, HistoryList/Item, PreviewPanel, SnippetsPanel, NotesPanel, …
│   │       ├── hooks/       # useClipboardHistory, useFuzzySearch, useSnippets, useNotes, useKeyboardNav
│   │       └── lib/         # ipc.ts, types.ts, calc.ts (Alfred-Style-Evaluator), format.ts
│   └── rust-lib/            # Geteilte Rust-App-Logik
│       ├── build.rs         # Linkt das macOS Vision-Framework für OCR
│       ├── models/
│       │   └── u2netp.onnx  # U²-Net Cutout-Model (~4,5 MB, Apache-2.0)
│       └── src/
│           ├── lib.rs                # Tauri-Builder, Plugin-/Tray-Setup, invoke_handler
│           ├── commands.rs           # alle #[tauri::command]-Wrapper
│           ├── models.rs             # ContentType / ClipEntry / NewClip + Caps
│           ├── db.rs                 # entries-Table, Hash-Dedup, Prune
│           ├── crypto.rs             # AES-256-GCM At-Rest-Encryption + OS-Keychain-Key
│           ├── snippets.rs           # snippets-Table, JSON-Upsert, Exakt-Abbreviation-Lookup
│           ├── seed.rs               # Default-AI-Prompt-Snippets — First-Launch-Seeder + `Restore defaults`-IPC
│           ├── seed/
│           │   └── ai_prompts.json   # 27 gebündelte AI-Prompts (~35 KB) — zur Compile-Zeit via include_str! eingelesen
│           ├── notes.rs              # notes-Table, Kategorien, save_from_clip
│           ├── backup.rs             # Full-App-Export/Import (versioniertes JSON)
│           ├── settings.rs           # Key/Value-Store (Expander-Hotkey + Zukunfts-Prefs)
│           ├── ui_state.rs           # suppress_hide-Flag für Native-Modal-Interaktion
│           ├── expander.rs           # Trigger-basierter Text-Expander (AX/UIA primary, Clipboard-Fallback)
│           ├── text_field/           # FieldAccess-Trait + macOS-AX + Windows-UIA-Implementierungen
│           ├── paste.rs              # write_to_clipboard + enigo-Paste-Shortcut
│           ├── hotkey.rs             # Global Ctrl+Space + Ctrl+Shift+O + Ctrl+Shift+S + Ctrl+Shift+C + Expander-Hotkey + Direct Slots
│           ├── clipboard_watcher.rs  # Event-getriebene Capture, RTF-Stripping (Image > Files-Priorität)
│           ├── recolor.rs            # Image-Tint (Lerp Target ↔ Weiß nach Per-Pixel-Luminanz)
│           ├── cutout.rs             # Legacy Chroma-Key-Cutout (als Fast-Path-Option behalten)
│           ├── cutout_ml.rs          # U²-Net-basierter Subject-Cutout via `ort` (ONNX Runtime)
│           ├── image_ops.rs          # `rz` Resize (Lanczos3) + `optim` PNG-Optimierung (oxipng)
│           ├── system_commands.rs    # `kill` / `reboot` / `shutdown` / `lock` (sysinfo + osascript)
│           ├── screen_picker.rs      # Farb-Eyedropper (NSColorSampler / GDI-Overlay)
│           ├── region_picker.rs      # screencapture-i (macOS) / GDI-Overlay (Windows) — OCR + Screenshot
│           ├── ocr.rs                # Apple Vision (macOS) / Windows.Media.Ocr (Windows)-Wrapper
│           └── screen_recording.rs   # macOS-Bildschirmaufnahme-TCC-Permission-API — gated OCR + Screenshot
├── win/                     # Windows-spezifische Bundle-Shell
│   ├── README.md            # Windows-Install- & Build-Details
│   ├── package.json         # Tauri-CLI-Entry
│   └── src-tauri/           # main.rs, Cargo.toml, tauri.conf.json, capabilities/, icons/
├── macos/                   # macOS-spezifische Bundle-Shell
│   ├── README.md            # macOS-Install, Gatekeeper, Accessibility, Troubleshooting
│   ├── package.json
│   └── src-tauri/           # entitlements.plist, tauri.conf.json (dmg+app), capabilities/
├── .github/
│   └── workflows/
│       ├── ci.yml           # Rust- + Frontend-Tests bei jedem Push/PR
│       └── release.yml      # Baut Bundles und published GitHub-Release bei v*-Tags
├── docs/
│   ├── spec.md              # Originale Produkt-Spezifikation
│   ├── snippets-import.md   # JSON-Snippet-Import — Schema, Semantik, Beispiele
│   ├── notes.md             # Notes-Feature — Kategorien, Edit-Semantik, IPC-Surface
│   ├── backup.md            # Full-App-Export/Import — Schema, Merge-Semantik, jq-Rezepte
│   ├── text-expander.md     # System-weiter Expander — Workflow, Hotkey-Format, Per-OS-Caveats
│   ├── colors.md            # Inline-Hex-Preview + Custom-HSV-Picker + System-Eyedropper
│   ├── ai-prompts.md        # 27 gebündelte Default-AI-Prompt-Snippets
│   ├── encryption.md        # AES-256-GCM At-Rest-Encryption — Threat-Model, Key-Storage, Migration
│   ├── RELEASING.md         # Release-Procedure
│   ├── ir-w1024.png         # Brand-Artwork — README-Hero + Inline-Image (1024×1024, ~1,9 MB)
│   └── examples/
│       └── snippets/        # 5 thematische JSON-Beispiele + eigene README
├── scripts/
│   ├── check.sh                      # cargo clippy + tsc + eslint
│   ├── install-macos.sh              # Idempotenter Build + stabiles-Zertifikat-Re-Sign + Install (erhält TCC-Grants über Rebuilds hinweg)
│   └── grant-permissions-macos.sh   # Geführtes Einmal-Setup für alle vier macOS-TCC-Berechtigungen
├── Cargo.toml               # Rust-Workspace (Members: core/rust-lib, win/src-tauri, macos/src-tauri)
├── pnpm-workspace.yaml      # pnpm-Workspace (core/frontend, win, macos)
└── package.json             # Root-Scripts: dev:{win,macos}, build:{win,macos}, lint, typecheck, format, test, check
```

## Quick Start

### Prerequisites

| Tool | Version | Hinweise |
|------|---------|----------|
| [Rust](https://rustup.rs/) | stable | MSVC-Toolchain auf Windows; `rustup component add clippy` ausführen |
| [Node.js](https://nodejs.org/) | 20+ | |
| [pnpm](https://pnpm.io/) | 10+ | `npm install -g pnpm` |

Plattformspezifische Prerequisites:
- **Windows** → [`win/README.md`](./win/README.md) (WiX, MSVC-Build-Tools, WebView2)
- **macOS** → [`macos/README.md`](./macos/README.md) (Xcode CLT, Gatekeeper, Accessibility-Permission)

### Install & run

```bash
pnpm install          # installiert den ganzen Workspace (CI nutzt --frozen-lockfile)

# Windows
pnpm dev:win          # tauri dev — Live-Reload
pnpm build:win        # → target/release/bundle/msi/InspectorRust_x.x.x_x64_en-US.msi

# macOS
pnpm dev:macos                      # tauri dev — Live-Reload
pnpm build:macos                    # → target/release/bundle/{macos/InspectorRust.app, dmg/InspectorRust_x.x.x_<arch>.dmg}
bash scripts/install-macos.sh             # build + re-sign (stabiles Zertifikat) + install + launch
bash scripts/install-macos.sh --reset    # …auch tccutil-reset stale TCC-Grants
bash scripts/grant-permissions-macos.sh  # geführtes Einmal-Setup für alle vier TCC-Berechtigungen
```

> Warum der `install-macos.sh`-Helper? Ohne Apple-Developer-ID ist jedes frische `pnpm build:macos` ad-hoc-signiert mit einem neuen cdhash, der alle vorherigen TCC-Grants invalidiert. Das Script erstellt einmalig ein stabiles selbstsigniertes Zertifikat und signiert jeden Build damit — TCC koppelt das Grant an die Designated Requirement (Bundle-ID + Zertifikat-Hash), nicht den cdhash, sodass **alle vier Berechtigungs-Grants jeden künftigen Rebuild überleben**. Voller Hintergrund: [`macos/README.md` — macOS-Berechtigungen](./macos/README.md#macos-permissions).

> Jede Plattform muss auf ihrem nativen Host gebaut werden (Windows für MSI, macOS für DMG/`.app`). Cross-Compilation wird nicht unterstützt.

### Snippet-Import

In Inspector Rust: Popup öffnen (`Ctrl+Space`) → **Snippets**-Tab → **Import** → eine `.json`-Datei auswählen. Der native File-Picker öffnet (NSOpenPanel auf macOS, OpenFileDialog auf Windows); existierende Abbreviations werden in-place upsert, sodass Re-Import derselben Datei idempotent ist.

**Ready-to-import-Samples** in [`docs/examples/snippets/`](./docs/examples/snippets/):

| Datei | Snippets | Thema |
|-------|----------|-------|
| [`getting-started.json`](./docs/examples/snippets/getting-started.json) | 3 | Adresse, E-Mail, deutsche Signatur — First-Import-Test |
| [`signatures.json`](./docs/examples/snippets/signatures.json) | 4 | E-Mail-Signaturen (DE/EN, kurz, OOO-Template) |
| [`dev.json`](./docs/examples/snippets/dev.json) | 8 | Shebang, MIT-Header, fn-Skeletons, gitignore, Commit-Msg |
| [`markdown.json`](./docs/examples/snippets/markdown.json) | 5 | Headings, Table, `<details>`, PR-Body |
| [`wrapped-form.json`](./docs/examples/snippets/wrapped-form.json) | 2 | Demonstriert `{ "snippets": [...] }`-Shape |

Siehe [`docs/snippets-import.md`](./docs/snippets-import.md) für das volle Schema, die Field-Semantik, das sqlite3+jq-Export-Rezept und Tips/Anti-Patterns.

### Notes & Backup

Notes haben ihren eigenen Tab; die Kategorien-Sidebar hat **+ New Note** und **Clear All**. Backup lebt jetzt im **Settings**-Tab.

- **Clipboard-Eintrag als Note speichern:** Hover über jede History-Zeile → Bookmark-Icon klicken → Eintrag landet im `Uncategorized`-Bucket des Notes-Tabs. Verschiebe ihn durch Editieren der Note in eine Kategorie.
- **Full-Backup exportieren:** Settings-Tab → **Backup & restore** → was exportieren ankreuzen (Clipboard-History / Snippets / Notes — alle Default an) → **Export…** → Pfad wählen. Inspector Rust schreibt eine einzelne JSON-Datei (Default-Name `inspector-rust-backup-<timestamp>.json`); ungeticked Sektionen werden als leere Arrays geschrieben, sodass du Snippets teilen kannst ohne deine Zwischenablage zu leaken.
- **Backup importieren:** Settings-Tab → **Backup & restore** → **Import…** → die JSON-Datei wählen. Snippets und History mergen nach ihren natürlichen Keys (Abbreviation / SHA-256-Hash); Notes werden appended. Notes- / Snippets- / History-Tabs aktualisieren sich automatisch.

Volle Feature-Referenz: [`docs/notes.md`](./docs/notes.md). Backup-Datei-Schema und Merge-Semantik: [`docs/backup.md`](./docs/backup.md).

### Tests

```bash
pnpm test               # Frontend-Unit-Tests (vitest + happy-dom) — 721 Tests
cargo test --workspace  # Rust-Unit-Tests — 477 Tests (hue, db, snippets, notes, backup, settings, expander, text_field, seed, hotkey-Parser, clipboard_watcher, models, recolor, cutout, cutout_ml, screen_record, audio_swap, media_trim, social_dl, audio, …)
```

Die gleichen Commands laufen in [GitHub-Actions-CI](./.github/workflows/ci.yml) bei jedem Push und PR.

### Statische Analyse

```bash
pnpm check            # cargo clippy (Workspace) + tsc --noEmit + eslint
```

## Bekannte Einschränkungen

| Einschränkung | Detail |
|---------------|--------|
| **Scope der At-Rest-Encryption** | Sensitive Inhalte (Clipboard-Text/-HTML/-RTF/-Bilder, Snippet-Bodies, Note-Bodies) sind AES-256-GCM-verschlüsselt at-rest mit einem per-Install zufälligen 256-Bit-Key (v0.6.0+). Key lebt im OS-Keychain; fällt zurück auf eine 0600-Keyfile im Data-Dir, wenn der Keychain nicht verfügbar ist. **Nicht verschlüsselt:** Timestamps, Content-Type-Tags, Dedup-Hashes, Snippet-Abbreviations, Note-Titles/-Kategorien — keines davon verrät Clipboard-Inhalt. Volle Referenz: [`docs/encryption.md`](./docs/encryption.md). |
| **Keine Sensitive-App-Detection** | Inspector Rust erfasst alles ohne Filterung. |
| **Kein Cloud-Sync** | Kein automatischer Sync oder Multi-Device-Support — aber der [Backup](./docs/backup.md)-Export/Import gibt dir eine portable JSON-Datei, die du manuell zwischen Maschinen bewegen kannst. |
| **File-Paste-Fallback** | Das Setzen von File-List-Clipboard-Payloads aus Rust wird nicht universell unterstützt; Inspector Rust fällt zurück darauf, die Newline-joined Liste der Pfade als Text zu pasten. |
| **Expander in Terminals (v0.64.0)** | Der *Abbreviation*-Hotkey (`Alt+1`) funktioniert seit v0.64.0 **auch in Terminals** (Terminal.app, iTerm2, kitty, …): ein passiver Tastatur-Tracker merkt sich die getippte Abkürzung und `Alt+1` expandiert sie aus dem Puffer (Blind-Backspace + Paste), ohne das Feld zu lesen. Alternativ weiterhin ein **Direct hotkey → snippet**-Slot oder das Popup. Electron-/Chromium-/Mac-Catalyst-Apps (WhatsApp, Slack, VS Code, …) werden zusätzlich via AX-select-then-paste unterstützt. |
| **macOS Bedienungshilfen** | Paste-Simulation (`enigo`), der system-weite Text-Expander und `freeze` brauchen Accessibility-Zugriff. Einmal in Systemeinstellungen → Datenschutz & Sicherheit → Bedienungshilfen erteilen; danach Inspector Rust einmal neu starten (der Settings-Tab bietet einen Ein-Klick-Neustart). Bei fehlendem Grant erscheint ein amber Banner beim nächsten Paste-Versuch oder Expander-Hotkey-Druck. |
| **macOS Bildschirmaufnahme** | OCR (`Ctrl+Shift+O`) und Screenshot-Region (`Ctrl+Shift+S`, v0.15.0+) brauchen beide Bildschirmaufnahme-Zugriff — `screencapture -i` wird Inspector Rust zugeordnet und macOS verweigert es ohne das Grant. Pre-checked via `CGPreflightScreenCaptureAccess`; fehlende Permission öffnet das Popup + zeigt ein amber Banner (v0.11.0). Der Eyedropper (`Ctrl+Shift+C`) braucht **keine** Bildschirmaufnahme. |
| **macOS Automation → Finder** | Finder-Selektion (`Ctrl+Shift+F`) und Markdown→PDF (`Ctrl+Shift+M`) senden Apple Events an Finder. Die erste Nutzung löst den Automation-Prompt aus; Allow klicken. |
| **macOS unsigned Build** | Release-Builds sind nicht notarisiert. macOS warnt eventuell "unidentified developer" — Rechtsklick auf die App und **Open** wählen, um Gatekeeper beim ersten Launch zu umgehen. |
| **macOS Rebuild ⇒ Re-Grant (abgemildert)** | Plain Ad-hoc-Builds ändern den `cdhash` bei jedem Rebuild und würden TCC-Grants invalidieren. `scripts/install-macos.sh` signiert mit einem stabilen selbstsignierten Zertifikat — TCC-Grants überleben jeden künftigen Rebuild. Ein Re-Grant ist nur nötig, wenn erstmalig von einem Plain-Build auf das Install-Script umgestellt wird. Vollständige Details: [`macos/README.md`](./macos/README.md#why-grants-survive-every-rebuild--the-stable-self-signed-certificate). |
| **Windows-OCR-Sprachpakete** | Die Windows-OCR-Engine (`Windows.Media.Ocr`) nutzt die in Einstellungen → Zeit & Sprache → Sprache installierten Sprachpakete. Ist für den auf dem Bildschirm dargestellten Text kein Paket installiert, schlägt die Engine mit einer beschreibenden Fehlermeldung fehl. Das fehlende Paket in den Windows-Einstellungen hinzufügen und erneut versuchen. |
| **Linux: Wayland-Shortcuts & Tools** | Globale Tauri-Shortcuts erhalten unter GNOME/Wayland oft keine Tastenevents — Inspector Rust registriert beim ersten Start automatisch GNOME/Cinnamon-`gsettings`-Custom-Keybindings (CLI-Flags `--toggle-popup` / `--ocr` / `--screenshot` / `--pick-color`). Region-Capture braucht `grim`+`slurp` (Wayland) bzw. `scrot` (X11), OCR braucht `tesseract` + Sprachpakete. Eyedropper und der In-Place-AX-Expander sind unter Linux noch nicht verfügbar (Clipboard-Paste-Fallback). Details: [`linux/README.md`](./linux/README.md). |

## Beiträge

Beiträge sind willkommen — siehe [`CONTRIBUTING.md`](./CONTRIBUTING.md) für den Dev-Workflow, Code-Style und wie man IPC-Commands oder neue Plattform-Shells hinzufügt.

## Releasing

Push ein `v*`-Tag, um den [Release-Workflow](https://github.com/pepperonas/inspector-rust/actions/workflows/release.yml) zu triggern, der die Windows-, macOS- und Linux-Bundles baut und an einen GitHub-Release attached. Volle Procedure (Version-Bumps, Pre-Flight-Checks, Troubleshooting) in [`docs/RELEASING.md`](./docs/RELEASING.md).

## Changelog

Siehe [`CHANGELOG.md`](./CHANGELOG.md) — jeder Release ist dokumentiert mit dem, was hinzugefügt, gefixt wurde, und etwaige bekannte Issues zu der Zeit.

## Entwickler

- **Martin Pfeffer** — Autor & Maintainer
- Kudos 2 Daniel

## License

[MIT](./LICENSE) — © 2026 Martin Pfeffer

A private open-source side project — built on weekends and evenings, made with ❤️.

Brewed and shipped from Berlin 🍻
