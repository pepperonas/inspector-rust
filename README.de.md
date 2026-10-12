<div align="right">

[🇬🇧 English](./README.md) · **🇩🇪 Deutsch**

</div>

<div align="center">
  <img src="docs/ir.png?v=6" alt="Inspector Rust — keyboard-first Desktop-Toolkit" width="600" />

  # Inspector Rust 🕵️‍♂️

  > **Ein Hotkey, dein ganzer Werkzeugkasten: Clipboard-History, Text-Expander, Launcher, Screenshots & OCR, Screen-Recording, systemweiter EQ und 70+ Power-Befehle — in einem einzigen nativen Popup für macOS, Windows 11 und Linux. Kein Electron, keine Cloud, keine Telemetrie.**

  Drück **`Ctrl+Space`** irgendwo → rahmenloses Popup über dem aktiven Monitor → Suche durch 1 000 deduplizierte Clipboard-Einträge → Enter pastet zurück in die zuvor fokussierte App. Ganzer Loop unter 200 ms, unter 50 MB RAM, AES-256-GCM-verschlüsselt at-rest mit Keys im OS-Keychain. **Gebaut für die Art Mensch, die schon Muskelgedächtnis für drei Clipboard-Manager hat und von allen genervt ist.**

  <h3>🌐 <a href="https://inspector-rust.celox.io">inspector-rust.celox.io</a> — die Produktseite</h3>

  <p>
    <a href="https://inspector-rust.celox.io">
      <img src=".github/badges/produktseite-inspector-rust-celox-io-6366f1-6cb63a84.svg" height="80" alt="Produktseite: inspector-rust.celox.io" />
    </a>
    <a href="https://inspector-rust.celox.io/download">
      <img src=".github/badges/download-fu-r-dein-system-22c55e-5fb36b6a.svg" height="80" alt="Download für dein System" />
    </a>
  </p>

  <em>Bilder aller Funktionen, die komplette Funktionsliste, das Changelog, Prüfsummen und ein Download mit einem Klick, der die richtige Datei für dein System wählt.</em>

  <p>
    <a href="https://www.paypal.com/donate/?business=martin.pfeffer@celox.io&item_name=Inspector+Rust&currency_code=EUR">
      <img src=".github/badges/spendier-dem-dev-einen-kaffee-spenden-via-paypal-f280d186.svg" height="80" alt="Spenden via PayPal" />
    </a>
  </p>

  <em>Kostenlos, Open-Source, null Telemetrie — eine Person, viele durchgemachte Nächte und eine unvernünftige Menge Espresso. Wenn Inspector Rust dir gerade ein paar hundert Tastenanschläge gespart hat, <a href="https://www.paypal.com/donate/?business=martin.pfeffer@celox.io&item_name=Inspector+Rust&currency_code=EUR">wirf einen Kaffee in die Kasse</a> ☕ — jede Tasse wird wortwörtlich zum nächsten Befehl.</em>

  ### ✨ Was es kann (in kurz)

  *Grob sortiert nach Alltagsnutzen × Engineering-Aufwand dahinter — Flagship-Features zuerst, Easter-Eggs zuletzt.*

  - 📋 **Clipboard-History** — Text, RTF, HTML, PNG, Datei-Listen; 1 000 Einträge per SHA-256 dedupliziert; **Substring-Suche** while-you-type; jeden Clip pinnen + mit Notiz versehen.
  - 🎯 **Text-Expander — 4 Modi**: passive **Auto-Expansion** (aText-Stil — expandiert beim Tippen, kein Hotkey) · In-Popup-Suche · systemweiter Hotkey (AX/UIA In-Place-Replace + Electron-Fallback) · Direkt-Hotkey → Snippet-Slots (geht auch in Terminals). **Dynamische Platzhalter** zur Paste-Zeit: `{date}` / `{date:%d.%m.%Y}`, `{time}`, `{datetime}`, `{clipboard}`, `{cursor}`, `{{`/`}}`.
  - 🧮 **Inline-Rechner** (`2+2`, `sqrt(144)`, Hex/Bit-Ops; Slot-Machine-Reveal), **Einheiten- / Basis- / Zeit-Konverter** (`5 km in mi`, `0xff in dec`, `1700000000 as date`) und **Farb-Konverter** (`#hex` / `rgb()` / `hsl()` in jede Richtung).
  - 🎚️ **Systemweites Audio-EQ — `boom`** (macOS · Windows via [Equalizer APO](https://sourceforge.net/projects/equalizerapo/)) — ein **10-Band-Grafik-Equalizer + Volume-Boost + 20 Presets** auf den *gesamten* System-Sound, plus **5 Enhancement-Effekte** (Bass · Clarity · Fidelity · Ambience-Stereo-Verbreiterung · Night-Kompressor fürs leise Hören), mit Live-Input/Output-Pegelmetern und **wahrnehmungsgerechter Lautstärke-Kurve** (die Standard-Kurve des virtuellen Treibers machte alles unter 40 % fast unhörbar; boom wendet jetzt einen echten Power-Taper an — der Regler fühlt sich wie echte Hardware an). Installiert einen kleinen virtuellen Audio-Treiber direkt aus dem Panel (ein Klick), matcht die Sample-Rate deines Geräts und **folgt deinem Ausgabegerät live** (inkl. Bluetooth). **Akku-schonend:** nach 60 s Stille suspendiert sich die Audio-Bridge selbst, damit der Mac normal schlafen kann — und ist in Millisekunden wieder da, sobald irgendetwas abspielt.
  - 🪟 **Fenster-Management** (macOS, opt-in) — zieh ein Fenster an einen Bildschirmrand zum **Snappen** (linke/rechte Hälfte · oben = maximieren, Magnet-Stil), oder schweb über den grünen Zoom-Button für eine **Moom-artige Palette**: Preset-Layouts (⌥ für Viertel) + ein **Honigwaben-Raster** (16×10 Standard, bis 24 — abgerundete Hexagons mit magnetischem Hover, leuchtender Auswahl und Live-Maßanzeige), über das du ziehst, um das Fenster in jede Bildschirmregion zu legen — mit Live-Umriss-Vorschau auf dem Screen.
  - 📸 **Screenshots — CleanShot-X-Stil**: Region (`Ctrl+Shift+S`) · Vollbild · aktives Fenster · Selbstauslöser · Wiederholen; schwebendes Vorschau-HUD; **Annotations-Editor** (Pfeil / Linie / Text / Rechteck / Ellipse / Highlight / Blur / Schwärzen / nummerierte Schritt-Badges); **an Bildschirm pinnen**. Dateinamen enthalten die Quell-App.
  - 🎥 **Bildschirmaufnahme** (`Ctrl+Shift+Alt+S`) — Region ziehen → Audio wählen (System / Mikro / beide) → 3-2-1 → **MP4 (H.264)** nach Downloads; schwebende Leiste mit **Pause/Resume**; Multi-Monitor; System-Audio routet automatisch über ein Loopback. Braucht ffmpeg.
  - 🔍 **Bildschirm-OCR** (`Ctrl+Shift+O`) — Apple Vision (macOS) / WinRT (Windows) / Tesseract (Linux). PDF-Qualität-Texterkennung ins Clipboard.
  - 🎬 **Medien-Tools** — **Download** von YouTube / Instagram / TikTok / Facebook (Video oder Audio — einfach eine URL einfügen; Tab toggelt bei YouTube), mit optionalem **Ausschnitt** unter den Buttons: gelbe Griffe im QuickTime-Stil, per Ohr über einen kleinen Audio-Proxy scrubben — der Download lädt dann **nur den Schnitt** (framegenau; 20 s aus einem 83-Minuten-Set kommen als ~1,2 MiB statt Hunderter MB); **Audio-Swap** (`Ctrl+Shift+Alt+M`) ersetzt oder mischt den Ton eines Videos mit einer lokalen Datei oder einem YouTube-Track; **Trim** von Audio/Video (`trim`) verlustfrei-schnell oder frame-genau. Brauchen ffmpeg / yt-dlp.
  - ⏱️ **Zeiterfassung / Timesheet** (`track on/off`; `track` oder **`Ctrl+Shift+T`**; macOS) — opt-in, event-basierte App-Nutzungserfassung per Fensterfokus mit rückwirkender Idle-Auto-Pause; ein editierbarer **Timesheet-Tab** mit Tages-/Wochen-Ansicht, Inline-SVG-Charts (Timeline · App-Donut · Kategorien · Projekte), **manueller Pause/Weiter-Taste**, CSV- + eigenständigem HTML-Export im sichtbaren Umfang (Tag oder Mo–So-Woche), **wochenweitem Aufräumen** und globalem **Tracking-Hotkey** (`Ctrl+Shift+Alt+T`, umbelegbar); erkennt **Claude-Code**-Nutzung pro Projekt (Zeit + Tokens); optionale **Browser-Extension** (nur Loopback-Socket). Fenstertitel + URLs at-rest verschlüsselt.
  - 📊 **System-Stats** (`stats`) — Live-Inline-Dashboard: CPU (gesamt + pro Kern), Speicher + Swap, **Akku & Leistungsaufnahme in Watt**, Temperaturen + **Lüfter-RPM** (SMC / hwmon), Disks, Live-Netzwerk-Durchsatz, Uptime. **Live ↔ History**-Umschalter mit Linien-Charts pro Metrik (1 h / 6 h / 24 h / 7 d).
  - 📈 **KI-Nutzungslimits — `limits` / `quota`** *(v0.190.0+)* — die Limits deines **Claude**-Abos wie auf claude.ai (Sitzung, Woche, Woche je Modell, zusätzliche Nutzung, Wochenanteil je Bereich), je mit Balken, Prozent und Rücksetzzeit (relativ + Berliner Zeit). Darunter **Codex** (5-Stunden-Fenster und Woche aus seinen lokalen Session-Dateien — ohne Netz) und **Antigravity** (Kontingent erschöpft / wieder frei ab, aus seinen Logs). Der Claude-Code-Login wird **nur gelesen** — nie erneuert, nie zurückgeschrieben; fragt alle 5 Min. nur bei offenem Panel, bei 429 mit Backoff. [Details](./docs/claude-limits.md)
  - 🤖 **KI-Tasks — `task` / `ki`** *(v0.191.0+)* — beschreibe in eigenen Worten, was passieren soll; **Claude** (API-Schlüssel oder das lokale Claude Code), **Gemini** oder **ChatGPT** schreibt das Skript (zsh, Bash, Python, AppleScript — unter Windows PowerShell), du prüfst es (riskante Zeilen markiert, Überarbeitungen als Diff) und **gibst genau diese Fassung frei**. Danach läuft es von selbst — alle N Minuten, zur Uhrzeit an gewählten Wochentagen, wenn Dateien in einem Ordner landen, beim App-Start oder nach dem Aufwachen — mit Zeitlimit, Lauf-Protokoll und einem Schalter, der alles pausiert. Schlüssel bleiben im Schlüsselbund. [Details](./docs/ai-tasks.md)
  - 👾 **Live-Retro-Overlay — `8bit` / `16bit`** *(v0.190.0+, macOS; Windows nicht im Betrieb geprüft)* — dein ganzer Bildschirm live in Retro-Optik, während du normal weiterarbeitest: Pixelung, feste Palette (NES, C64, PICO-8, Game Boy, CGA, Graustufen) oder reduzierte Farbtiefe pro Kanal (SNES, Mega Drive, Amiga), Bayer-Dithering, Scanlines, CRT-Krümmung. Ein **Fokus-Modus** zeichnet das aktive Fenster feiner, eine **Cursor-Lupe** zeigt den Bereich unter der Maus im Detail, so bleibt Text lesbar; Stufe 2 bringt Retro-Fensterrahmen mit Pixelschrift-Titeln und einen Sprite-Cursor. Einstellungs-Panel mit Live-Vorschau und Presets; ⌃⇧⌥8 schaltet von überall an und aus. ScreenCaptureKit → Metal-Shader, ohne Kopie. [Details](./docs/8bit.md)
  [![Gesten-Schutz](.github/badges/gestures-trace-replay-guard-1f6feb-613d0271.svg)](./docs/gestures.md)
  - ☀️ **Monitor-Helligkeit** (`brightness` / `bri`) — Slider inline in der Vorschau für interne *und* externe Displays (**↑↓** Monitor wählen, **←→** anpassen). Software-(Gamma-)Dimming auf macOS + Windows, Hardware-DDC/CI auf Linux. Auf **EDR-fähigen Macs** (14"/16" MBP XDR, Pro Display XDR) läuft *derselbe* Slider **über 100 %** hinaus und hebt das Display in seinen **Extra-Helligkeits-Bereich (EDR/XDR)** — Vivid-Stil, bis ~7× — via Multiply-Blend-Metal-Overlay; macOS drosselt thermisch automatisch (gleicher Pfad wie HDR-Video, innerhalb der Spezifikation).
  - 💡 **Philips Hue** (`hue`) — steuere deine Lampen inline: Alle-Lampen an/aus + Helligkeit, Helligkeit pro Lampe, 8 Farb-Preset-Swatches auf Farb-Bulbs. Plus eine **Beat-Sync**-Disco, die die Lampen zur Musik vom Mikro pulsen lässt. Lokales LAN-Pairing (Discovery oder IP + Link-Button); keine Cloud.
  - 🖐️ **Touchpad-Gesten** (opt-in) — **3-Finger-Swipe** hoch/runter für Lautstärke (konsistente 5-%-Raster-Schritte), **3-Finger-Tap** zum Stummschalten, plus **Tip-Tap-Tab-Wechsel** (macOS, opt-in): **einen** Finger auflegen, mit einem zweiten rechts/links daneben tippen → nächster/voriger Tab — dabei sendet IR automatisch **den passenden Shortcut jeder App** (Ctrl+Tab für Browser/Terminals/Finder, ⌘⌥→/← für VS Code/Cursor, ⇧⌘]/[ für JetBrains/Xcode — layoutbewusst aufgelöst, z. B. ⌥6 auf Deutsch; erweiterbar per `tab-shortcuts.json` im App-Datenordner, ohne Rebuild). **Gesten-Schutz**: jede Geste läuft vor dem Auslösen durch vier Filterebenen — Handballen und Daumen zählen nie als Finger, Berührungen, die in den Randzonen **landen**, werden ignoriert, ein Tastendruck kurz vor oder während der Berührung sperrt Lautstärke und Stumm (250 ms nach einer einzelnen Taste, 600 ms in einer Tastenfolge; Entmuten und Tab-Wechsel werden nie gesperrt), und ein Swipe, zu dem ein weiterer Finger dazukommt, wird verworfen. Das **`gestures`**-Panel zeigt das Trackpad live, hält die Schwellen je Gerät (eingebaut / extern), listet die letzten 20 Entscheidungen samt der Ebene, die gestoppt hat, **kalibriert** Handballen- und Daumen-Schwellen für dein Trackpad in rund 20 Sekunden und nimmt Spuren zum Nachspielen auf; **⌃⇧⌥G** sichert die letzten 3 Sekunden, wenn etwas von selbst ausgelöst hat. Das Lautstärke-Overlay steht nach der letzten Geste 2,2 s und schließt nie, solange die Maus darauf liegt; eine Erfassung, die taub wird (nach Ruhezustand, Monitorwechsel oder still), baut sich binnen etwa einer Minute selbst neu auf. macOS über die private MultitouchSupport-API (schluckt den Swipe, damit das Fenster darunter nicht scrollt); Windows Precision Touchpad mit allen vier Ebenen über das Confidence-Bit des Touchpads (noch nicht auf echter Hardware getestet); Linux libinput mit eingeschaltetem Disable-while-typing. Details: [docs/gestures.md](./docs/gestures.md).
  - 🔐 **2FA / TOTP-Manager** — tippe `2fa` *oder* `otp` für den TOTP-Tresor — **einfach lostippen filtert die Liste** (fuzzy, Enter kopiert den Code des Top-Treffers); `otp <issuer>` / `2fa <issuer>` für sofortige OTP-Autovervollständigung mit Live-30-Sekunden-Countdown, Enter kopiert den Token; **`2fa add [issuer]`** springt direkt ins Anlege-Formular (Issuer vorbefüllt). **Hinzufügen / Bearbeiten / Löschen, Drag-Umsortieren und Dedup beim Import**; importiert Google Authenticator / Aegis / 2FAS / **OTPManager (macOS)** / `otpauth` — einfügen *oder* Export-Datei aufs Overlay ziehen. Secrets verschlüsselt, überqueren nie die IPC-Grenze.
  - 🔊 **Audio-Ausgabe** (`sound` / `audio`) — Inline-Picker zum Umschalten des System-Standard-Ausgabegeräts (macOS · Windows · Linux).
  - 🎵 **Song-Erkennung — `shazam`** — tippe `shazam`, und es nimmt ~10 s vom Mikrofon auf, erzeugt nativ in Rust einen Shazam-Audio-Fingerprint (keine Datei, kein ffmpeg) und identifiziert den Track: Cover, Titel, Künstler, Album, Genre, Jahr + Link zum Öffnen in Shazam. `shazam history` öffnet die Liste erkannter Songs. Bit-genau gegen die Referenz und end-to-end gegen den echten Dienst verifiziert.
  - 🕵️ **Netzwerk-Monitor — `snitch`** (macOS) — listet jede App mit aktiver Verbindung und lässt dich ihren Internetzugang abschalten (**best-effort**: ein Hintergrund-Watcher schiebt die Server-IPs einer geblockten App in die pf-Firewall — ein Admin-Prompt, keine echte Firewall; ein echter per-App-Filter bräuchte ein Apple-System-Extension-Entitlement, das eine selbstsignierte App nicht hat). **`snitch map`** zeichnet deine laufenden Outbound-Verbindungen auf eine Offline-Punkt-Weltkarte — Verbindungen, über die **gerade Daten fließen, leuchten grün mit animierten Paketen** entlang eines Bogens von deinem Standort — jeder Server nach Land/Stadt/ISP verortet (nur öffentliche IPs — LAN-Adressen verlassen den Rechner nie). Beim Tippen von `snitch` erscheinen Blocker und Karte als wählbare Zeilen.
  - 🧹 **Aufräumen** (`clean`) — Speicherplatz freigeben durch Löschen von Cache-/Log-/Temp-Dateien in bekannt-sicheren Ordnern. Auf **Standard** wird das komplette Nutzer-Cache-Verzeichnis gefegt (`~/Library/Caches` — oft viele GB); opt-in **Aggressive** ergänzt Dev-Tool-Caches (npm/pnpm/Gradle/Cargo inkl. Sources), Xcode-Build-Caches und alte Papierkorb-Einträge. Enter öffnet einen **interaktiven Kategorien-Picker** (Größen, Dateianzahl, größte Dateien — du wählst genau, was weg soll) — inklusive **Duplikaten in Downloads** (inhalts-gehasht, das älteste Exemplar bleibt immer erhalten), **alten Installern** (dmg/pkg/iso), **Editor-Caches** (VS Code / Cursor), **Xcode DerivedData + iOS DeviceSupport** und dem **Docker-Build-Cache** (via `docker builder prune`) — riskante Kategorien sind vorab abgewählt, Nutzerdateien nie Default; strikte Allowlist, Symlinks werden nie verfolgt; Safe / Standard / Aggressive.
  - 🎨 **Farbpipette** (`Ctrl+Shift+C`) — eigene Bildschirm-Lupe mit **Live-Hex unter der Vergrößerung** (macOS) / GDI-Overlay (Windows); Hex direkt ins Clipboard.
  - 🖼️ **Bild-Tools** — Recolor (Logo-Tint), ML-**Freisteller** (U²-Net ONNX, 4,5 MB eingebettet), Lanczos3-**Resize** (`rz`) + **Optimieren** (`optim`, oxipng) auf die Finder-Auswahl oder das Clipboard-Bild.
  - 📁 **Finder-Auswahl-Aktionen** (`Ctrl+Shift+F`, macOS) — Batch-Resize / -Optim / -Freisteller / -Öffnen auf alles, was du im Finder ausgewählt hast.
  - 📄 **Markdown → PDF** (`Ctrl+Shift+M` / `md2pdf`, macOS) — konvertiert die im Finder ausgewählten `.md`-Dateien in-process zu PDF; keine CLI-Tools nötig.
  - 🚀 **App-Launcher** (Spotlight-artig, macOS) — App-Name fuzzy matchen, echtes Icon in der Zeile, Enter startet. Aktiviert eine bereits laufende Instanz statt ein Duplikat zu starten.
  - 🔳 **QR-Code** (`qr <text>`) — Live-Vorschau im Panel; Enter kopiert das PNG ins Clipboard.
  - 🛠️ **Dev-Quick-Tools** — `uuid [n]` · `slug` · `hash` (SHA-256) · `json` (Clipboard pretty-printen) · `jwt` (Clipboard dekodieren) → Clipboard.
  - 🎲 **Fake-Testdaten — `faker`** *(v0.84.270+)* — 70+ Generatoren (Namen, E-Mails, Adressen, Telefon, Firmen, Finanzen, Lorem, Datum, Zahlen, UUID/…, plus Composite-Records **person / user / address / order**) in 14 Locales. `faker` allein listet sie mit Live-Samples; `faker person 50 --csv @de` → 50 deutsche Datensätze als CSV im Clipboard, ein Enter. `--json` / `--sql` / `--ts`, `faker int 1..100`, `--seed=` reproduzierbar, ⌘/Ctrl+R würfelt neu, `faker tpl "{name} <{email}>"`. Ehrlicher Locale-Fallback (nicht unterstützt → EN, sichtbar). Auch `{faker:first_name}` in Snippets. Siehe [docs/faker.md](./docs/faker.md).
  - 🛡️ **Security-Command-Builder — `sec`** *(v0.84.271+)* — geführte Command-Builder für **nmap · sqlmap · feroxbuster · John**. Preset wählen, Ziel eintragen; Inspector Rust baut die korrekte (sh/bash-gequotete) Kommandozeile mit Flag-Erklärungstabelle — `nmap service 10.0.0.5` → `nmap -sV -sC 10.0.0.5`. **Enter kopiert; ⌘/Ctrl+Enter öffnet dein Terminal** mit dem eingefügten Command (macOS, opt-in, standardmäßig nicht abgeschickt). **Scannt nie selbst** — kein Subprozess, kein Netzwerk. Nur autorisierte Ziele. Siehe [docs/security-builder.md](./docs/security-builder.md).
  - 🌐 **Web-Such-Bangs** — `g` · `ddg` · `gh` · `yt` · `npm` · `crates` · `so` · `wiki` `<query>` öffnen die Suche der jeweiligen Seite.
  - 🥁 **BPM-Detektor** (`bpm`) — Live-Beat-Erkennung über das Mikro mit animiertem AAA-Visualizer. Nimmt nativ (in Rust) auf, sodass der Start die Wiedergabe anderer Apps nie unterbricht. Der kleine Bruder **`dezibel` / `db`** zeigt die Live-Lautstärke in dB (SPL-Schätzung, dBFS + 90) mit derselben Leucht-und-Meter-Animation.
  - 🏁 **CPU-Benchmark** (`benchmark` / `performance`) — sieben Disziplinen nach Geekbench-Vorbild, ein- und mehrkernig, bewertet gegen eine gemessene Referenz. Tippen zeigt erst die Vorschau; gemessen wird erst nach Bestätigung. Läufe liegen als JSON — ein Lauf von **einem anderen Rechner oder OS** lässt sich importieren und mit Deltas nebeneinanderlegen (Unterschiede unterhalb der Streuung sind als Rauschen markiert). Export als HTML/PDF.
  - 📑 **Signierte Reports** — jeder exportierte Report (LoC, PageSpeed, Repo-Statistik, Benchmark, Bruno, Zeiterfassung; HTML wie PDF) trägt **Unterschrift und Vektor-Stempel** im geteilten druckfertigen Design — ein Blick über alle.
  - 💸 **Bruno (Brutto/Netto)** — deutscher Netto-Rechner 2025 als Suchleisten-Command, für **Angestellte UND Selbständige**: `bruno 60000` (Gehalt) oder `bruno 80000f` / `bruno 90000-15000f` (Gewinn bzw. Einnahmen − Ausgaben) mit freiwilliger GKV oder PKV-Fixbeitrag, Gewerbesteuer inkl. §35-Anrechnung, Grund-/Splittingtarif. **Tab wechselt Angestellter ↔ Unternehmer** auf der Zeile; Shift+Enter kopiert die komplette Aufstellung, und die Vorschau **exportiert sie als HTML oder PDF** im geteilten Report-Design. Smarte Defaults + Pro-User-Override in den Einstellungen.
  - ⚙️ **Power-Commands** — die Suchleiste parst Dutzende Shell-artige Commands: Übersetzen (`tr` / `tren` / `trde` / `trde2it` / …), System (`kill` / `lock` / `reboot` / `shutdown` / `mute` / `freeze`), `rnd` / `random` (Würfeln), `timer` / `alarm <HH:MM>`, `touch` / `mkdir` / `terminal` (im offenen Finder-Ordner), `rmvvls`, `pwgen`, `meme [query]`, `calendar [monat jahr]` (Monatsansicht — welcher Wochentag war Datum X?) — plus jedes oben genannte Command. Fuzzy-gematcht, immer über den Clips, mit rotem Akzent gerendert. Ein **`?`** hinter einem Command (oder `?` allein) zeigt die volle **Inline-Hilfe** — Argumente, Beispiele, Tipps — direkt im Preview (auf Englisch).
  - 📓 **Snippets** (27 mitgelieferte KI-Prompts + 255 Material-Farben, **in Gruppen organisiert** — filtern, zuweisen, anlegen/umbenennen/sortieren/löschen) · **Notes** (persistente Bookmarks) · **Backup** (komplette App als eine Datei — inkl. 2FA + alle Settings + Snippet-Gruppen, Timesheet optional; optional passwort-verschlüsselt, Import mit Inline-Passwortabfrage).
  - 🟢 **Keep-alive & Wakelock** — `wakelock on/off` (Alias `caffeine`) hält die Maschine wach (pulsierende Footer-LED + On-Screen-Toast); **„Always keep running"** (Einstellungen → Startup) startet die App nativ neu, falls sie je beendet/gekillt wird.
  - 🔒 **Local-first** — null Netzwerk-Calls, null Account; Daten nur unter `~/Library/Application Support/InspectorRust/history.db`, AES-256-GCM-verschlüsselt mit Keys im OS-Keychain.
  - 🎮 **Versteckte Spiele** — fünf Easter-Egg-Triggerwörter. Du wirst sie finden.

  ### 🧰 Tech-Stack

  Tauri 2 (WebView2 / WKWebView) · Rust-Workspace (`core/rust-lib` geteilt, 2-Zeilen-Per-OS-Bundle-Shells) · React 19 + TypeScript 5 + Tailwind v4 + Vite 7 · Helligkeit via CoreGraphics/GDI-Gamma + DDC/CI (`ddc-hi`). **5551 Unit-Tests (2155 Rust + 3396 Frontend).** MIT-lizenziert.

  <!-- ── Headline-Kennzahlen — XXL Hero-Badges ─────────────────── -->
  <p>
    <a href="https://github.com/pepperonas/inspector-rust" title="Codezeilen (Rust + TypeScript Quellcode)">
      <img src="https://img.shields.io/badge/lines%20of%20code-~236k-2b3137?style=for-the-badge&logo=rust&logoColor=white" height="64" alt="Lines of code" />
    </a>
    &nbsp;
    <a href="https://github.com/pepperonas/inspector-rust/actions/workflows/ci.yml" title="Unit-Tests — 2155 Rust + 3396 Frontend, alle grün">
      <img src="https://img.shields.io/badge/unit%20tests-5551%20passing-2ea043?style=for-the-badge&logo=vitest&logoColor=white" height="64" alt="Unit tests" />
    </a>
  </p>

  <!-- ── Highlights — prominente (for-the-badge) Badges ────────── -->
  <p>
    <a href="./LICENSE"><img src=".github/badges/license-mit-green-b039fe28.svg" alt="MIT License" /></a>
    <a href="https://github.com/pepperonas/inspector-rust/releases/latest"><img src="https://img.shields.io/github/v/release/pepperonas/inspector-rust?style=for-the-badge&label=download&color=1f6feb" alt="Latest release" /></a>
    <a href="#"><img src=".github/badges/platforms-macos-windows-linux-8957e5-eebf454c.svg" alt="Plattformen: macOS, Windows, Linux" /></a>
    <a href="https://tauri.app"><img src=".github/badges/tauri-2-ffc131-91c709ef.svg" alt="Tauri 2" /></a>
    <a href="https://rustup.rs"><img src=".github/badges/rust-stable-ce422b-8ecee2ac.svg" alt="Rust" /></a>
    <a href="#"><img src=".github/badges/privacy-100-local-no-telemetry-2ea043-4ee84631.svg" alt="Privatsphäre: 100% lokal, keine Telemetrie" /></a>
    <a href="./scripts/check.sh"><img src=".github/badges/clippy-d-warnings-ce422b-4fa2c396.svg" alt="clippy -D warnings" /></a>
  </p>

  <!-- ── Status / release ─────────────────────────────────────── -->
  [![CI](https://img.shields.io/github/actions/workflow/status/pepperonas/inspector-rust/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/pepperonas/inspector-rust/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/actions/workflow/status/pepperonas/inspector-rust/release.yml?branch=main&style=flat-square&label=release)](https://github.com/pepperonas/inspector-rust/actions/workflows/release.yml)
  [![Last commit](https://img.shields.io/github/last-commit/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Issues](https://img.shields.io/github/issues/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/issues)
  [![Stars](https://img.shields.io/github/stars/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/stargazers)
  [![Maintenance](.github/badges/maintained-yes-brightgreen-f6453ee2.svg)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Unit tests](https://img.shields.io/badge/unit%20tests-5551%20(2155%20Rust%20%2B%203396%20TS)-success?style=flat-square)](https://github.com/pepperonas/inspector-rust/actions/workflows/ci.yml)
  [![PRs welcome](.github/badges/prs-welcome-brightgreen-bf350bde.svg)](./CONTRIBUTING.md)
  [![Code Style](.github/badges/code-style-clippy-eslint-orange-216d70a2.svg)](./scripts/check.sh)
  [![Commands](https://img.shields.io/badge/commands-77-4f46e5?style=flat-square)](#commands)
  [![Docs](https://img.shields.io/badge/docs-38%20pages-0ea5e9?style=flat-square)](./docs)
  [![Rust modules](https://img.shields.io/badge/rust%20modules-107-CE422B?style=flat-square&logo=rust&logoColor=white)](./core/rust-lib/src)
  [![Crates](https://img.shields.io/badge/crates-869-brightgreen?style=flat-square&logo=rust&logoColor=white)](./Cargo.lock)
  [![IPC commands](https://img.shields.io/badge/IPC%20commands-497-7c3aed?style=flat-square&logo=tauri&logoColor=white)](./core/rust-lib/src/lib.rs)
  [![UI components](https://img.shields.io/badge/UI%20components-106-61DAFB?style=flat-square&logo=react&logoColor=black)](./core/frontend/src/components)
  [![Test suites](https://img.shields.io/badge/test%20suites-321-2ea043?style=flat-square&logo=vitest&logoColor=white)](#testing)
  [![Settings sections](https://img.shields.io/badge/settings%20sections-44-6366F1?style=flat-square)](./core/frontend/src/lib/settings-sections.ts)
  [![Tab completion](https://img.shields.io/badge/Tab%20completion-24%20commands-e11d48?style=flat-square)](./core/frontend/src/lib/arg-complete.ts)
  [![Rebindable hotkeys](https://img.shields.io/badge/rebindable%20hotkeys-13-0ea5e9?style=flat-square)](./core/rust-lib/src/hotkey.rs)
  [![Hidden triggers](https://img.shields.io/badge/hidden%20triggers-11-ff69b4?style=flat-square)](./core/frontend/src/lib/feature-extras.ts)
  [![Frontend modules](https://img.shields.io/badge/frontend%20modules-118-3178C6?style=flat-square&logo=typescript&logoColor=white)](./core/frontend/src/lib)
  [![Releases](https://img.shields.io/badge/releases-573-8957e5?style=flat-square)](./CHANGELOG.md)
  [![License](.github/badges/license-mit-blue-d5dc0172.svg)](./LICENSE)
  [![Tauri](.github/badges/tauri-2-24c8db-31b76b30.svg)](https://tauri.app)
  [![Offline first](.github/badges/offline-first-334155-52fd5d0d.svg)](#privacy)
  [![No telemetry](.github/badges/telemetry-none-334155-3bacf58a.svg)](#privacy)
  [![Downloads](https://img.shields.io/github/downloads/pepperonas/inspector-rust/total?style=flat-square&label=downloads&color=8957e5)](https://github.com/pepperonas/inspector-rust/releases)
  [![Code size](https://img.shields.io/github/languages/code-size/pepperonas/inspector-rust?style=flat-square)](#)
  [![Commit activity](https://img.shields.io/github/commit-activity/m/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Last commit](https://img.shields.io/github/last-commit/pepperonas/inspector-rust?style=flat-square)](https://github.com/pepperonas/inspector-rust/commits/main)
  [![Top language](https://img.shields.io/github/languages/top/pepperonas/inspector-rust?style=flat-square)](#)
  [![Languages](https://img.shields.io/github/languages/count/pepperonas/inspector-rust?style=flat-square&label=languages)](#)
  [![Release date](https://img.shields.io/github/release-date/pepperonas/inspector-rust?style=flat-square&label=released)](https://github.com/pepperonas/inspector-rust/releases/latest)
  [![Conventional Commits](.github/badges/conventional-commits-1-0-0-fe5196-e24d83f5.svg)](https://www.conventionalcommits.org)
  [![SemVer](.github/badges/semver-2-0-0-3f4551-d59624f2.svg)](https://semver.org)

  <!-- ── Platforms ────────────────────────────────────────────── -->
  [![Windows 11](.github/badges/windows-11-0078d4-642b03df.svg)](./win)
  [![macOS](.github/badges/macos-10-15-000000-77417231.svg)](./macos)
  [![Apple Silicon](.github/badges/arm64-apple-silicon-555555-19055736.svg)](./macos)
  [![x86_64](.github/badges/x86-64-supported-555555-5c9a49ab.svg)](#)
  [![Linux](.github/badges/linux-ubuntu-debian-brightgreen-b86fcee1.svg)](./linux/README.md)
  [![Cross-platform](.github/badges/cross-platform-3-oses-blueviolet-14ad8cf5.svg)](#)
  [![WebView2](.github/badges/windows-webview2-0078d4-0ec4594c.svg)](https://developer.microsoft.com/microsoft-edge/webview2/)
  [![WKWebView](.github/badges/macos-wkwebview-000000-5bffa9b1.svg)](#)

  <!-- ── Stack ────────────────────────────────────────────────── -->
  [![React 19](.github/badges/react-19-61dafb-a00f5f01.svg)](https://react.dev)
  [![TypeScript 5](.github/badges/typescript-5-3178c6-501e405e.svg)](https://www.typescriptlang.org)
  [![Vite 7](.github/badges/vite-7-646cff-15bda1c5.svg)](https://vitejs.dev)
  [![Tailwind CSS v4](.github/badges/tailwind-v4-38bdf8-2940e147.svg)](https://tailwindcss.com)
  [![pnpm](.github/badges/pnpm-10-f69220-cc4bedee.svg)](https://pnpm.io)
  [![Node 20](.github/badges/node-20-339933-5c511e46.svg)](https://nodejs.org)
  [![SQLite](.github/badges/sqlite-bundled-003b57-316a0f42.svg)](https://sqlite.org)
  [![ONNX Runtime](.github/badges/onnx-runtime-bundled-005ced-3123cd34.svg)](https://onnxruntime.ai)
  [![Apple Vision](.github/badges/apple-vision-live-text-000000-21f8fb08.svg)](#)
  [![U2Net](.github/badges/u2-net-cutout-orange-d38847fa.svg)](https://github.com/xuebinqin/U-2-Net)
  [![rusqlite](.github/badges/rusqlite-sqlite-003b57-966ba5f8.svg)](https://github.com/rusqlite/rusqlite)
  [![objc2](.github/badges/objc2-ffi-000000-05c8a133.svg)](https://github.com/madsmtm/objc2)
  [![windows-rs](.github/badges/windows-rs-win32-0078d4-a7192328.svg)](https://github.com/microsoft/windows-rs)
  [![CoreAudio](.github/badges/coreaudio-system-eq-000000-b5042676.svg)](#)
  [![ffmpeg](.github/badges/ffmpeg-record-trim-007808-8dfac5cb.svg)](https://ffmpeg.org)
  [![yt-dlp](.github/badges/yt-dlp-social-dl-ff0000-61f358cc.svg)](https://github.com/yt-dlp/yt-dlp)
  [![oxipng](.github/badges/oxipng-png-optim-orange-33b79f25.svg)](https://github.com/shssoichiro/oxipng)
  [![BlackHole](.github/badges/blackhole-audio-loopback-1db954-541ced65.svg)](https://github.com/ExistentialAudio/BlackHole)
  [![Metal](.github/badges/metal-edr-overlay-000000-84975e44.svg)](#)
  [![Core Image](.github/badges/core-image-blend-filter-000000-7c38ebef.svg)](#)
  [![AppKit](.github/badges/appkit-nswindow-000000-b1bce763.svg)](#)
  [![CoreGraphics](.github/badges/coregraphics-gamma-edr-000000-811a3f9a.svg)](#)
  [![CoreAudio](.github/badges/coreaudio-device-switch-000000-29b0f4ea.svg)](#)
  [![Rust 2021](.github/badges/edition-2021-ce422b-ba509f58.svg)](https://doc.rust-lang.org/edition-guide/)
  [![sysinfo](.github/badges/sysinfo-system-metrics-ce422b-b7437353.svg)](https://github.com/GuillaumeGomez/sysinfo)
  [![starship-battery](.github/badges/battery-power-draw-ce422b-6a87a4cd.svg)](https://github.com/starship/rust-battery)
  [![chrono](.github/badges/chrono-date-time-ce422b-6b392cfb.svg)](https://github.com/chronotope/chrono)
  [![ureq](.github/badges/ureq-lan-http-ce422b-10a8e30f.svg)](https://github.com/algesten/ureq)
  [![image](.github/badges/image-resize-optim-ce422b-c7edf5c5.svg)](https://github.com/image-rs/image)
  [![parking_lot](.github/badges/parking-lot-mutex-ce422b-a8af7ba9.svg)](https://github.com/Amanieu/parking_lot)
  [![argon2](.github/badges/argon2-id-darkgreen-e3db5afa.svg)](https://github.com/RustCrypto/password-hashes)

  <!-- ── Security & ergonomics ───────────────────────────────── -->
  [![AES-256-GCM](.github/badges/encryption-aes-256-gcm-darkgreen-667015ba.svg)](./docs/encryption.md)
  [![Keychain-backed](.github/badges/keys-os-keychain-555555-b1da80c8.svg)](./docs/encryption.md)
  [![Auto-backup](.github/badges/auto-backup-scheduled-encrypted-restorable-1f6fe-e4bc2f26.svg)](./docs/auto-backup.md)
  [![Local-first](.github/badges/local-first-yes-brightgreen-d87c8929.svg)](#)
  [![No telemetry](.github/badges/telemetry-none-brightgreen-512e0154.svg)](#)
  [![Offline](.github/badges/network-not-required-brightgreen-aef30406.svg)](#)
  [![Power user](.github/badges/audience-power-users-blueviolet-59003dcf.svg)](#)
  [![Keyboard-first](.github/badges/ux-keyboard-first-blueviolet-f135efa3.svg)](#)

  <!-- ── Quality ─────────────────────────────────────────────── -->
  [![ESLint](.github/badges/eslint-flat-config-4b32c3-cc02ea79.svg)](https://eslint.org)
  [![Vitest](.github/badges/vitest-3-6e9f18-d14e6138.svg)](https://vitest.dev)
  [![cargo test](https://img.shields.io/badge/cargo%20test-2155%20passing-success?style=flat-square&logo=rust&logoColor=white)](#)
  [![vitest](https://img.shields.io/badge/vitest-3396%20passing-success?style=flat-square&logo=vitest&logoColor=white)](#)
  [![cargo clippy](.github/badges/cargo-clippy-d-warnings-success-0e95b602.svg)](#)
  [![tsc strict](.github/badges/tsc-strict-3178c6-8998462e.svg)](#)
  [![Prettier](.github/badges/code-style-prettier-f7b93e-52e0150f.svg)](https://prettier.io)

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
  [![Made with love](.github/badges/made-with-red-b5450299.svg)](#)

  <!-- ── Architecture & build ────────────────────────────────── -->
  [![Monorepo](.github/badges/repo-pnpm-workspace-f69220-acdc227c.svg)](./pnpm-workspace.yaml)
  [![Workspace crates](.github/badges/cargo-workspace-3-crates-ce422b-8764ff93.svg)](./Cargo.toml)
  [![Single binary](.github/badges/distribution-single-binary-blue-59a8ba62.svg)](#)
  [![Native](.github/badges/no-electron-success-e4560d9e.svg)](#)
  [![Memory](.github/badges/memory-50-mb-blue-8f1d089d.svg)](#)
  [![Cold start](.github/badges/cold-start-200-ms-blue-5d570842.svg)](#)
  [![MSI size](.github/badges/msi-5-mb-blue-de237bbe.svg)](#)
  [![DMG size](.github/badges/dmg-5-mb-blue-d5d7063e.svg)](#)
  [![exe size](.github/badges/exe-14-mb-blue-2fcaaf46.svg)](#)

  <!-- ── Features (numerical) ────────────────────────────────── -->
  [![Tests](https://img.shields.io/badge/tests-5551%20passing-success?style=flat-square)](#)
  [![IPC commands](https://img.shields.io/badge/IPC%20commands-497-blueviolet?style=flat-square)](./core/rust-lib/src/commands.rs)
  [![Search-bar commands](https://img.shields.io/badge/search--bar%20commands-77-blueviolet?style=flat-square)](./core/rust-lib/src/commands.rs)
  [![Tauri events](https://img.shields.io/badge/events-64-blueviolet?style=flat-square)](#)
  [![Rust modules](https://img.shields.io/badge/Rust%20modules-107-CE422B?style=flat-square&logo=rust&logoColor=white)](./core/rust-lib/src)
  [![Snippets](.github/badges/ai-prompts-27-bundled-blueviolet-3f4859ed.svg)](./docs/ai-prompts.md)
  [![Media](.github/badges/media-record-download-trim-swap-ce422b-b9dff908.svg)](#)
  [![Motion](.github/badges/motion-material-3-expressive-blueviolet-8a8ecfba.svg)](#)
  [![Tabs](.github/badges/popup-tabs-6-blueviolet-15b9fb59.svg)](#)
  [![DB tables](https://img.shields.io/badge/SQLite%20tables-26-003B57?style=flat-square&logo=sqlite&logoColor=white)](./docs/encryption.md)
  [![Global shortcuts](https://img.shields.io/badge/global%20hotkeys-15-blueviolet?style=flat-square)](#)
  [![Snippet expansion modes](.github/badges/expansion-modes-4-blueviolet-8a752ab1.svg)](./docs/text-expander.md)
  [![Image formats](.github/badges/image-formats-5-blueviolet-73e0101a.svg)](#)
  [![Time tracking](.github/badges/timesheet-event-based-encrypted-ce422b-9f407ee0.svg)](./docs/timesheet.md)
  [![Privacy](.github/badges/privacy-offline-no-telemetry-success-1b1e254a.svg)](./docs/encryption.md)
  [![Rust LoC](https://img.shields.io/badge/Rust-~124k%20LoC-CE422B?style=flat-square&logo=rust&logoColor=white)](./core/rust-lib/src)
  [![TS LoC](https://img.shields.io/badge/TypeScript-~112k%20LoC-3178C6?style=flat-square&logo=typescript&logoColor=white)](./core/frontend/src)
  [![Features](https://img.shields.io/badge/features-213-0ea5e9?style=flat-square)](./features.txt)
  [![EDR headroom](.github/badges/xdr-up-to-1600-nits-ffb300-683a54d2.svg)](#)
  [![Audio presets](.github/badges/boom-20-eq-presets-1db954-38c8049c.svg)](#)
  [![Material colours](.github/badges/snippets-255-colours-blueviolet-b1f62fa5.svg)](#)
  [![Web bangs](.github/badges/web-search-9-bangs-1f6feb-f973893a.svg)](#)
  [![Capture modes](.github/badges/screenshot-region-full-window-e11d48-928a09a9.svg)](#)
  [![Inline panels](.github/badges/inline-panels-stats-hue-boom-sound-1f6feb-b01a4ebf.svg)](#)
  [![Annotation tools](.github/badges/screenshot-editor-9-tools-e11d48-d67331d3.svg)](#)

  <!-- ── Features ────────────────────────────────────────────── -->
  [![Clipboard history](.github/badges/clipboard-history-pins-1f6feb-bc2e730c.svg)](#)
  [![Power commands](.github/badges/search-bar-power-commands-e11d48-a007b87a.svg)](#)
  [![Text expander](.github/badges/snippets-text-expander-1f6feb-a74785dd.svg)](#)
  [![Screen OCR](.github/badges/ocr-screen-region-000000-a307045b.svg)](#)
  [![Screen recording](.github/badges/screen-record-trim-e11d48-8afb7f42.svg)](#)
  [![System EQ](.github/badges/boom-system-audio-eq-1db954-eec4ae56.svg)](#)
  [![2FA / TOTP](.github/badges/2fa-totp-rfc-6238-darkgreen-e90c60ee.svg)](#)
  [![Eyedropper](.github/badges/color-eyedropper-loupe-blueviolet-aa80792f.svg)](#)
  [![Markdown → PDF](.github/badges/markdown-pdf-purple-6928c6ff.svg)](#)
  [![Time tracking](.github/badges/timesheet-time-tracking-1f6feb-85068cce.svg)](#)
  [![Hidden games](.github/badges/easter-eggs-5-games-ff69b4-15f40057.svg)](#)
  [![Material 3](.github/badges/material-3-expressive-757575-2ea3855d.svg)](https://m3.material.io)
  [![EDR brightness](.github/badges/brightness-edr-xdr-boost-ffb300-5691b9ae.svg)](#)
  [![Philips Hue](.github/badges/philips-hue-lan-control-00a1e9-addd4467.svg)](#)
  [![Trackpad gestures](.github/badges/trackpad-gestures-blueviolet-0c4dc13e.svg)](#)
  [![Window snapping](.github/badges/windows-snap-palette-1f6feb-77d29747.svg)](#)
  [![Keep-alive](.github/badges/keep-alive-auto-relaunch-2ea043-f091b5ea.svg)](#)
  [![System stats](.github/badges/stats-live-dashboard-1f6feb-4f46ca71.svg)](#)
  [![Live uptime](.github/badges/uptime-live-readout-1f6feb-97941f38.svg)](#)
  [![QR codes](.github/badges/qr-generate-copy-000000-c8ed1225.svg)](#)
  [![BPM detector](.github/badges/bpm-mic-detector-ff69b4-e5c9cb21.svg)](#)
  [![Net-pay calc](.github/badges/bruno-net-pay-calc-1f6feb-466e0a8b.svg)](#)
  [![Password gen](.github/badges/pwgen-csprng-darkgreen-6c5f248f.svg)](#)
  [![Cache cleaner](.github/badges/clean-cache-sweep-1f6feb-d67834f0.svg)](#)
  [![Audio output](.github/badges/sound-output-switch-1db954-647dd073.svg)](#)
  [![Image tools](.github/badges/images-resize-optim-cutout-e11d48-59377806.svg)](#)
  [![App launcher](.github/badges/launcher-fuzzy-apps-1f6feb-cd1bcf18.svg)](#)
  [![Input lock](.github/badges/freeze-input-lock-e11d48-d39b81bb.svg)](#)
  [![Wake-lock](.github/badges/caffeine-keep-awake-blueviolet-4dd976e4.svg)](#)
  [![Timers](.github/badges/timer-alarm-1f6feb-a15bcae0.svg)](#)
  [![Translate](.github/badges/translate-en-de-it-es-pl-1f6feb-480d64cc.svg)](#)
  [![ASCII banners](.github/badges/figlet-ascii-banners-550-fonts-e11d48-7dae8c92.svg)](./docs/figlet.md)
  [![Song recognition](.github/badges/shazam-song-recognition-1db954-861aedf6.svg)](#)
  [![Network monitor](.github/badges/snitch-per-app-net-monitor-e11d48-08801e43.svg)](#)
  [![Connections map](.github/badges/snitch-map-world-map-live-arcs-2ea043-0bf09d1b.svg)](#)
  [![Fake test data](.github/badges/faker-70-generators-14-locales-blueviolet-e74098b5.svg)](./docs/faker.md)
  [![Security builders](.github/badges/sec-nmap-sqlmap-ferox-john-darkgreen-b03152f0.svg)](./docs/security-builder.md)
  [![Bluetooth capture](.github/badges/btsniff-hci-att-gatt-analyzer-darkgreen-932bb24a.svg)](./docs/bluetooth-capture.md)
  [![Spotify](.github/badges/spotify-local-play-history-control-1db954-7ff7a308.svg)](./docs/spotify.md)
  [![GitHub analytics](.github/badges/github-followers-stars-history-181717-c8e249ca.svg)](./docs/github.md)
  [![Transcribe](.github/badges/transcribe-whisper-offline-e11d48-9550b3ca.svg)](./docs/transcribe.md)
  [![Tab](.github/badges/tab-ghost-text-typo-fix-habits-first-e11d48-736f466c.svg)](./core/frontend/src/lib/arg-complete.ts)
  [![Pulse](.github/badges/pulse-memory-pressure-ssd-writes-1f6feb-067c3729.svg)](./docs/pulse.md)
  [![Speedtest](.github/badges/speedtest-down-up-ping-history-1f6feb-ad037ade.svg)](./docs/speedtest.md)
  [![AI tasks](.github/badges/task-ai-scripts-approved-by-hash-blueviolet-70a9f13f.svg)](./docs/ai-tasks.md)
  [![Disk](.github/badges/disk-usage-sunburst-e11d48-bc57b8b4.svg)](./docs/disk.md)
  [![adb](.github/badges/adb-android-control-3ddc84-ff9c224d.svg)](./docs/adb.md)
  [![Bluetooth](.github/badges/bluetooth-devices-connect-0082fc-c7a270f9.svg)](#commands)
  [![PageSpeed](.github/badges/pagespeed-desktop-mobile-4285f4-97e1c8de.svg)](#commands)
  [![Benchmark](.github/badges/benchmark-7-cpu-disciplines-ce422b-5e0b5f9a.svg)](#commands)
  [![Dezibel](.github/badges/dezibel-live-db-meter-ff69b4-b2aec004.svg)](#commands)
  [![Iris](.github/badges/iris-loudness-vignette-e11d48-2175e8cf.svg)](#commands)
  [![Lumen](.github/badges/lumen-ambient-light-ffb300-34b03bd7.svg)](#commands)
  [![Clock](.github/badges/clock-world-clock-1f6feb-155737c1.svg)](#commands)
  [![Alias](.github/badges/alias-shell-alias-builder-1f6feb-55f6c7f9.svg)](#commands)
  [![Tokens](.github/badges/tokens-claude-code-usage-d97757-7def765f.svg)](#commands)
  [![No sleep](.github/badges/nosleep-ac-sleep-profile-blueviolet-e4b83ddd.svg)](#commands)
  [![Cursor wrap](.github/badges/cursor-wrap-infinity-monitor-blueviolet-b87e9a0f.svg)](#commands)
  [![Device sync](.github/badges/device-sync-shared-folder-encrypted-darkgreen-e985ab18.svg)](#commands)
  [![Clown](.github/badges/clown-silly-text-styles-ff69b4-8f02e35a.svg)](#commands)
  [![Random](.github/badges/rnd-dice-ranges-1f6feb-80017ab3.svg)](#commands)
  [![Mail check](.github/badges/mailcheck-syntax-mx-smtp-0ea5e9-d75dfc36.svg)](./docs/mailcheck.md)
  [![Inline help](.github/badges/inline-command-help-e11d48-df469d81.svg)](./docs/inline-help.md)
  [![Lineage rails](.github/badges/clipboard-copy-shapes-lineage-1f6feb-54257641.svg)](./docs/clipboard-shapes.md)
  [![Live translation](.github/badges/translate-live-preview-keyless-blueviolet-7570e737.svg)](./docs/translation.md)
  [![Calendar](.github/badges/calendar-month-view-weekday-lookup-1f6feb-e7a8668a.svg)](#)
  [![Social download](.github/badges/download-youtube-insta-tiktok-fb-ff0000-0587d947.svg)](#)
  [![Audio swap](.github/badges/audio-swap-replace-mix-video-audio-e11d48-544d8a55.svg)](#)
  [![Media trim](.github/badges/trim-lossless-frame-accurate-e11d48-981529b0.svg)](#)
  [![Meme picker](.github/badges/meme-gif-library-picker-ff69b4-8236f98d.svg)](#)
  [![Smart actions](.github/badges/smart-actions-url-mail-tel-maps-qr-1f6feb-698e3a2c.svg)](#)
  [![Inline calculator](.github/badges/calc-inline-slot-machine-reveal-e11d48-8711d843.svg)](#)
  [![Unit converter](.github/badges/convert-units-currencies-crypto-1f6feb-f074c988.svg)](./docs/convert.md)
  [![Color converter](.github/badges/colors-hex-rgb-hsl-blueviolet-c74869b3.svg)](./docs/colors.md)
  [![Dev tools](.github/badges/dev-uuid-slug-hash-json-jwt-1f6feb-1fe34a90.svg)](#)
  [![Web bangs](.github/badges/bangs-g-gh-yt-npm-so-wiki-1f6feb-fe849bf7.svg)](#)
  [![Finder actions](.github/badges/finder-selection-actions-000000-4a65b78f.svg)](#)
  [![Text transforms](.github/badges/text-case-base64-url-slug-1f6feb-e2348b5a.svg)](#)
  [![Snippet groups](.github/badges/snippets-groups-versioning-1f6feb-43bca795.svg)](./docs/snippets-import.md)
  [![Auto-expansion](.github/badges/auto-expand-atext-style-1f6feb-a383542a.svg)](./docs/text-expander.md)
  [![Notes](.github/badges/notes-persistent-bookmarks-1f6feb-1e05592d.svg)](./docs/notes.md)
  [![Encrypted backup](.github/badges/backup-argon2id-aes-256-darkgreen-b804e45c.svg)](./docs/backup.md)
  [![Clipboard privacy](.github/badges/privacy-app-exclude-auto-clear-darkgreen-1bd4a2fc.svg)](#)
  [![Kill picker](.github/badges/kill-live-process-picker-red-456a34e0.svg)](#)
  [![Screenshot editor](.github/badges/screenshots-annotate-pin-e11d48-06aa32bb.svg)](#)
  [![Disco beat-sync](.github/badges/disco-beat-sync-lights-ff69b4-cdb0037a.svg)](#)
  [![Tip-tap tabs](.github/badges/tip-tap-per-app-tab-switch-blueviolet-87acaffc.svg)](#)
  [![Rebindable hotkeys](.github/badges/hotkeys-all-rebindable-blueviolet-81a6ebe7.svg)](#)
  [![Status toasts](.github/badges/toasts-cursor-monitor-hud-1f6feb-68725c74.svg)](#)
  [![Dice](.github/badges/rnd-csprng-dice-ff69b4-687ec4c7.svg)](#)
  [![Multi-monitor](.github/badges/multi-monitor-cursor-aware-blueviolet-2b0e57f6.svg)](#)
  [![Themes](.github/badges/theme-dark-light-system-757575-adda4981.svg)](#)
  [![Sound cues](.github/badges/sounds-tactile-cues-1f6feb-0815dc0d.svg)](#)
  [![TOTP import](.github/badges/2fa-import-gauth-aegis-2fas-otpmanager-darkgreen-cb3bfff7.svg)](#)
  [![Song history](.github/badges/shazam-local-history-1db954-e349ae51.svg)](#)
  [![Popup sizes](.github/badges/popup-s-m-l-blueviolet-82f3ffad.svg)](#)

  <!-- ── Standards / conventions ─────────────────────────────── -->
  [![SemVer](.github/badges/semver-2-0-blue-f19d1ebb.svg)](https://semver.org)
  [![Keep a Changelog](.github/badges/changelog-keep-a-changelog-orange-492d7209.svg)](https://keepachangelog.com)
  [![Conventional Commits](.github/badges/commits-conventional-orange-97b5747e.svg)](https://www.conventionalcommits.org)
  [![ARIA](.github/badges/a11y-keyboard-first-blueviolet-91d2cf9a.svg)](#)
  [![ADRs in CHANGELOG](.github/badges/adrs-in-changelog-orange-3c13acaa.svg)](./CHANGELOG.md)

  <!-- ── Permissions / OS surfaces ───────────────────────────── -->
  [![macOS TCC: Accessibility](.github/badges/macos-tcc-accessibility-000000-8505f299.svg)](./macos/README.md#macos-permissions)
  [![macOS TCC: Screen Recording](.github/badges/macos-tcc-screen-recording-000000-99350d94.svg)](./macos/README.md#macos-permissions)
  [![macOS TCC: Automation](.github/badges/macos-tcc-automation-finder-000000-b3380393.svg)](./macos/README.md#macos-permissions)
  [![macOS TCC: Microphone](.github/badges/macos-tcc-microphone-opt-000000-74b25b2c.svg)](./macos/README.md#macos-permissions)
  [![Autostart](.github/badges/autostart-launchagent-regrun-blue-9abe3d89.svg)](./CHANGELOG.md)
  [![Tray icon](.github/badges/ui-tray-resident-blue-985ee2cb.svg)](#)

  <!-- ── Tech (extended) ─────────────────────────────────────── -->
  [![rusqlite](.github/badges/rusqlite-bundled-ce422b-0cec01bf.svg)](https://crates.io/crates/rusqlite)
  [![enigo](.github/badges/enigo-paste-sim-ce422b-73dbbf7c.svg)](https://crates.io/crates/enigo)
  [![clipboard-rs](.github/badges/clipboard-rs-event-driven-ce422b-3adf87ff.svg)](https://crates.io/crates/clipboard-rs)
  [![ort](.github/badges/ort-onnx-runtime-ce422b-4a3fbd92.svg)](https://crates.io/crates/ort)
  [![ring](.github/badges/ring-aes-256-gcm-ce422b-5379ad07.svg)](https://crates.io/crates/ring)
  [![objc2](.github/badges/objc2-vision-ffi-ce422b-2ac75e25.svg)](https://crates.io/crates/objc2)
  [![lucide-react](.github/badges/icons-lucide-react-f56565-36356404.svg)](https://lucide.dev)
  [![react-virtual](.github/badges/list-react-virtual-ff4154-1a977da1.svg)](https://tanstack.com/virtual)

  <!-- ── Vibes ───────────────────────────────────────────────── -->
  [![Inspired by Alfred](.github/badges/inspired-by-alfred-blueviolet-6e6cf32d.svg)](#)
  [![Mouse-free](.github/badges/mouse-not-required-brightgreen-d9154372.svg)](#)
  [![Self-hosted](.github/badges/data-on-your-disk-brightgreen-b3f91a94.svg)](#)
  [![Free forever](.github/badges/free-forever-brightgreen-6a05ce24.svg)](./LICENSE)
  [![Made in Germany](.github/badges/made-in-germany-ffce00-a8a00675.svg)](#)

  <!-- ── Neu dazugekommen ────────────────────────────────────── -->
  [![Wetter](.github/badges/wetter-vorschau-animiert-4aa3e8-c218a996.svg)](#)
  [![Equalizer](.github/badges/equalizer-28-band-mikro-visualizer-e11d48-2f19fc81.svg)](#)
  [![Konfigurierbarer Verlauf](.github/badges/verlauf-limit-50-100k-1f6feb-e93dff2d.svg)](#)
  [![Esc-only + Enter-Paste](.github/badges/popup-esc-only-enter-paste-blueviolet-c98db761.svg)](#)
  [![Cloud-Sync](.github/badges/snippets-cloud-sync-cue-1f6feb-96541022.svg)](#)
  [![Songtexte](.github/badges/shazam-songtexte-lrclib-1db954-e8496cb4.svg)](https://lrclib.net)
  [![In Spotify öffnen](.github/badges/shazam-in-spotify-o-ffnen-1db954-ea81f00d.svg)](#)
  [![CRT-Animation](.github/badges/popup-crt-einschalt-animation-ff69b4-f611befc.svg)](#)
  [![View Transitions](.github/badges/motion-view-transitions-blueviolet-0146e90e.svg)](#)
  [![Reduced Motion](.github/badges/a11y-prefers-reduced-motion-2ea043-9c753526.svg)](#)
  [![Snippet-Versionierung](.github/badges/snippets-inhalts-versionierung-1f6feb-4518bd4f.svg)](./docs/snippets-import.md)
  [![KI-Nutzungslimits](.github/badges/limits-claude-codex-antigravity-d97757-a929253b.svg)](./docs/claude-limits.md)
  [![Retro-Overlay](.github/badges/8bit-live-retro-overlay-7c3aed-3517146c.svg)](./docs/8bit.md)
  [![Login nur lesend](.github/badges/limits-token-nur-lesend-2ea043-35c1e0c3.svg)](./docs/claude-limits.md)
  [![Codex lokal](.github/badges/codex-lokale-session-dateien-412991-86c99d86.svg)](./docs/claude-limits.md)

  <!-- ── Tech (noch mehr) ────────────────────────────────────── -->
  [![OpenWeatherMap](.github/badges/openweather-vorhersage-api-eb6e4b-895d1916.svg)](https://openweathermap.org)
  [![Anthropic usage API](.github/badges/anthropic-oauth-usage-api-d97757-48bd99ac.svg)](./docs/claude-limits.md)
  [![Metal](.github/badges/macos-screencapturekit-metal-000000-1d917697.svg)](./docs/8bit.md)
  [![Frankfurter](.github/badges/rates-ecb-via-frankfurter-1f6feb-60cab905.svg)](./docs/convert.md)
  [![CoinGecko](.github/badges/crypto-coingecko-8dc63f-eebe81dc.svg)](./docs/convert.md)
  [![ip-api](.github/badges/ip-api-geolokalisierung-1f6feb-a668c862.svg)](https://ip-api.com)
  [![rustfft](.github/badges/rustfft-fft-ce422b-ac09cfb3.svg)](https://crates.io/crates/rustfft)
  [![cpal](.github/badges/cpal-natives-mikro-ce422b-5c57d215.svg)](https://crates.io/crates/cpal)
  [![Web Audio](.github/badges/web-audio-analysernode-ff3e00-da65a1e7.svg)](#)
  [![Google Translate](.github/badges/u-bersetzen-google-gtx-4285f4-6b349cb4.svg)](./docs/translation.md)
  [![MyMemory](.github/badges/u-bersetzen-mymemory-fallback-1f6feb-c08ca961.svg)](./docs/translation.md)
  [![lrclib](.github/badges/songtexte-lrclib-net-1db954-4fa46b1a.svg)](https://lrclib.net)
  [![figlet-rs](.github/badges/figlet-rs-ascii-ce422b-92fd1218.svg)](https://crates.io/crates/figlet-rs)
  [![pulldown-cmark](.github/badges/pulldown-cmark-markdown-ce422b-501b92cc.svg)](https://crates.io/crates/pulldown-cmark)
  [![Simple Icons](.github/badges/brand-icons-simple-icons-111111-bc7dfff8.svg)](https://simpleicons.org)
  [![CGEventTap](.github/badges/macos-cgeventtap-000000-a548d4c7.svg)](#)
  [![MultitouchSupport](.github/badges/macos-multitouchsupport-000000-1ab91085.svg)](#)
  [![Equalizer APO](.github/badges/windows-equalizer-apo-0078d4-aeae0943.svg)](https://sourceforge.net/projects/equalizerapo/)

  <!-- ── Testing (erweitert) ─────────────────────────────────── -->
  [![happy-dom](.github/badges/happy-dom-test-env-6e9f18-dfc69fac.svg)](https://github.com/capricorn86/happy-dom)
  [![Coverage-Tooling](.github/badges/coverage-llvm-cov-v8-2ea043-77505ee2.svg)](#)
  [![lib-Coverage](.github/badges/lib-coverage-84-stmt-2ea043-87cc3054.svg)](#)
  [![GitHub Actions](.github/badges/ci-github-actions-2088ff-a4eb30ab.svg)](./.github/workflows/ci.yml)
  [![Deterministische Tests](.github/badges/tests-deterministisch-offline-success-cd03f3d0.svg)](#)
  [![Pure-Core-Testing](.github/badges/stil-pure-core-unit-tests-blueviolet-3c89e42c.svg)](#)

  <!-- ── Mehr Vibes ──────────────────────────────────────────── -->
  [![Buy me a coffee](.github/badges/support-buy-me-a-coffee-ffdd00-9024e534.svg)](https://www.paypal.com/donate/?business=martin.pfeffer@celox.io&item_name=Inspector+Rust&currency_code=EUR)
  [![celox.io](.github/badges/von-celox-io-0b57d0-c4d13fee.svg)](https://celox.io)
  [![Solo-Dev](.github/badges/gebaut-von-einem-menschen-blueviolet-8522b4a1.svg)](#)
  [![Espresso-betrieben](.github/badges/angetrieben-von-espresso-6f4e37-265ae0ef.svg)](#)
  [![Zero Config](.github/badges/setup-zero-config-brightgreen-a0d242b5.svg)](#)
  [![Kein Account](.github/badges/account-nicht-no-tig-brightgreen-2069ea26.svg)](#)
  [![100+ Features](https://img.shields.io/badge/features-213%2B-e11d48?style=flat-square)](./features.txt)
  [![Retro-Games](.github/badges/versteckt-pong-snake-invaders-flappy-ff69b4-5d4c2958.svg)](#)
  [![Auf LinkedIn teilen](.github/badges/teilen-auf-linkedin-0a66c2-c957a7ef.svg)](https://www.linkedin.com/)

  Drück `Ctrl+Space` → suchen → einfügen. Inspiriert von Alfreds Clipboard-Viewer auf macOS, eingedampft auf ein Tool, das du auf jeder Maschine behalten kannst.
</div>

---

## Screenshots

*Dark Theme auf macOS, mit Demo-Clipboard-History — Dummy-Daten, keine echten Clips.*

| | |
|---|---|
| ![weather — aktuelles Wetter + animierte Vorschau](docs/screenshots/weather.png) | ![boom — systemweiter Audio-EQ](docs/screenshots/boom.png) |
| **`weather`** — aktuelles Wetter, nächste 12 h + animierte 5-Tage-Vorschau | **`boom`** — systemweiter 10-Band-EQ + Presets + Volume-Boost |
| ![equalizer — Live-Mikro-Spektrum-Visualizer](docs/screenshots/equalizer.png) | ![bpm — Live-Mikro-Tempo-Detektor](docs/screenshots/bpm.png) |
| **`equalizer`** — Live-28-Band-Mikro-Spektrum + Beat-Reaktion | **`bpm`** — Live-Mikro-Tempo-Detektor (128 BPM · 99 % Konfidenz) |
| ![hue — Philips-Hue-Lampensteuerung](docs/screenshots/hue.png) | ![snitch map — Verbindungs-Weltkarte](docs/screenshots/snitch-map.png) |
| **`hue`** — Philips Hue: Farbe + Helligkeit pro Lampe | **`snitch map`** — laufende Outbound-Verbindungen auf einer Weltkarte |
| ![shazam — Songerkennung über das Mikro](docs/screenshots/shazam.png) | ![stats — Live-System-Dashboard](docs/screenshots/stats.png) |
| **`shazam`** — den laufenden Song über das Mikro erkennen | **`stats`** — Live-Dashboard für CPU / RAM / Akku / Netzwerk |
| ![calendar — Monatsansicht + Wochentag-Recherche](docs/screenshots/calendar.png) | ![brightness — Slider pro Monitor](docs/screenshots/brightness.png) |
| **`calendar`** — Monatsansicht + Wochentag-Recherche | **`brightness`** — Slider pro Monitor (+ EDR/XDR-Boost) |
| ![sound — Audio-Ausgabe-Wähler](docs/screenshots/sound.png) | ![uptime — Live-animierte Anzeige](docs/screenshots/uptime.png) |
| **`sound`** — Audio-Ausgabe-Wähler + Lautstärke-Slider | **`uptime`** — Live-animierte Uptime-Anzeige |
| ![Clipboard-History — Suche, Vorschau, Smart-Actions](docs/screenshots/history.png) | ![figlet — ASCII-Art-Banner-Galerie](docs/screenshots/figlet.png) |
| **Clipboard-History** — Suche beim Tippen, Live-Vorschau, Notizen + QR | **`figlet`** — ASCII-Banner-Galerie; jede Zeile rendert *deinen* Text |
| ![bruno — Brutto-Netto-Rechner](docs/screenshots/bruno.png) | ![Inline-Rechner](docs/screenshots/calc.png) |
| **`bruno`** — deutsche Brutto→Netto-Aufstellung | **Inline-Rechner** — Ausdruck tippen, Enter fügt das Ergebnis ein |
| ![Inline-Hilfe — Befehls-Index](docs/screenshots/help.png) | ![disk — Speicher-Sunburst à la DaisyDisk](docs/screenshots/disk.png) |
| **`?`** — eingebauter Befehls-Index & Inline-Hilfe | **`disk`** — Speicher-Sunburst; die Pfadleiste durchstöbert die ganze Platte |
| ![repo — Git-Aktivitätsstatistik](docs/screenshots/repo.png) | ![clock — Weltzeituhr](docs/screenshots/clock.png) |
| **`repo`** — Git- und GitHub-Aktivität (24 h / 7 T, Zeiträume, Heatmap, Hotspots), Export HTML/PDF, Klonen mit ⌘K | **`clock`** — Weltzeituhr; jede IANA-Zone, Sommerzeit macht das OS |
| ![clown — alberne Textstile](docs/screenshots/clown.png) | ![alias — geführter Alias-Baukasten + Verwaltung](docs/screenshots/alias.png) |
| **`clown`** — zwölf Textstile; du nimmst den, den du siehst | **`alias`** — Shell-Alias anlegen und bestehende verwalten |

## Download

**Aktueller Release:** [![Latest Release](https://img.shields.io/github/v/release/pepperonas/inspector-rust?style=flat-square&label=latest&color=green)](https://github.com/pepperonas/inspector-rust/releases/latest) — siehe [CHANGELOG](./CHANGELOG.md) für die Neuerungen.

**Website:** [inspector-rust.celox.io](https://inspector-rust.celox.io) (inkl. [aller Funktionen](https://inspector-rust.celox.io/#all-features), gespiegelt aus [`features.txt`](./features.txt), dazu Codezeilen und Unit-Test-Anzahl aus [`.github/repo-stats.json`](./.github/repo-stats.json) — beides folgt jedem Push binnen 15 Minuten) · **Download mit einem Klick für dein System:** [inspector-rust.celox.io/download](https://inspector-rust.celox.io/download)

| Plattform | Datei | Hinweise |
|-----------|-------|----------|
| **Windows 11 / 10** | [`InspectorRust_<ver>_x64_en-US.msi`](https://github.com/pepperonas/inspector-rust/releases/latest) | MSI-Installer — fügt Startmenü-Eintrag & Uninstaller hinzu |
| **Windows 11 / 10** | [`inspector-rust.exe`](https://github.com/pepperonas/inspector-rust/releases/latest) | Standalone-Exe — keine Installation nötig |
| **macOS 11+ (Apple Silicon)** | [`InspectorRust_<ver>_aarch64.dmg`](https://github.com/pepperonas/inspector-rust/releases/latest) | DMG für arm64-Macs |
| **macOS 11+ (Intel)** | [`InspectorRust_<ver>_x64.dmg`](https://github.com/pepperonas/inspector-rust/releases/latest) | DMG für Intel-Macs — alle Funktionen außer dem ML-Freistellen ([Grund](./macos/README.md#intel-macs-x86_64-no-ml-cut-out)) |
| **Linux x86-64** | [`InspectorRust_<ver>_amd64.deb`](https://github.com/pepperonas/inspector-rust/releases/latest) | Debian-Paket — gebaut auf Ubuntu 24.04, also Ubuntu 24.04+ / Debian 13 |
| **Linux x86-64** | [`InspectorRust_<ver>_amd64.AppImage`](https://github.com/pepperonas/inspector-rust/releases/latest) | Portabel; selbst bauen: [`linux/README.md`](./linux/README.md) |

> **macOS-Gatekeeper-Hinweis.** Releases sind ad-hoc-signiert, **nicht von Apple notarisiert**. Ein von GitHub geladenes DMG wird vom Browser unter Quarantäne gestellt; macOS behauptet dann, die App sei **"beschädigt und kann nicht geöffnet werden"** — ist sie nicht, und Rechtsklick → **Öffnen** hilft gegen genau diese Meldung *nicht*. App in den Programme-Ordner ziehen, dann das Attribut einmalig entfernen:
>
> ```bash
> xattr -dr com.apple.quarantine /Applications/InspectorRust.app
> ```
>
> (Bei einem lokal gebauten Build wird das Attribut nie gesetzt — nur für Downloads nötig.) Danach die TCC-Berechtigungen erteilen:
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

## Konfiguration

Alles wird im **Settings-Tab** des Popups konfiguriert (`Ctrl+Space` → Settings) — keine Config-Dateien. Das Wichtigste:

| Bereich | Einstellbar |
|---|---|
| **Popup-Hotkeys** | Haupt-Hotkey (Standard `Ctrl+Space`) + zweiter Clipboard-History-Hotkey (Standard `Ctrl+Shift+V`, abschaltbar) |
| **Globale Shortcuts** | Jeder Aktions-Hotkey (OCR, Screenshot, Eyedropper, Finder-Auswahl, Markdown→PDF, Recording, Audio-Swap, Timesheet, …) ist umbelegbar, mit Live-Kollisionsprüfung |
| **Text-Expander** | Abkürzungs-Hotkey, Direkt-Hotkey→Snippet-Slots, passive Auto-Expansion (aText-Stil), Trigger-/Case-Optionen |
| **Appearance** | Dark / Light / System-Theme + Popup-Größe (S / M / L) |
| **Clipboard-Privacy** | App-Ausschlussliste (z. B. Passwortmanager) + Auto-Clear-Timer |
| **Cleaning** | Safe / Standard / Aggressive, Mindestalter, Kategorie-Schalter, Dev-Projekt-Ordner |
| **Timesheet** | Idle-Schwelle, Aufbewahrung, Claude-Code-Erkennung, Privacy-Denylist |
| **Sounds** | Master-Schalter für alle Feedback-Sounds |
| **Command-Defaults** | Bruno (Steuerparameter), Faker, Figlet, Security-Builder, Meme-Bibliotheks-Ordner |
| **Startup** | Start bei Login + „Always keep running“-Auto-Relaunch |
| **Backup** | Voll-Export/-Import, optional passwortverschlüsselt (Argon2id + AES-256-GCM) |

Alle Daten liegen in einer SQLite-Datei — `~/Library/Application Support/InspectorRust/history.db` (macOS), `%APPDATA%\InspectorRust\history.db` (Windows), `~/.local/share/InspectorRust/history.db` (Linux) — sensible Spalten AES-256-GCM-verschlüsselt ([Details](./docs/encryption.md)).

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
| `Shift+↑` `Shift+↓` *(v0.22.0+)* | System-Lautstärke erhöhen / senken (±5 % pro Druck, rastet aufs 5-%-Raster) |
| `Enter` | Ausgewählten Eintrag pasten (respektiert das Plain-Text-Setting) |
| `Shift+Enter` | Mit Originalformatierung pasten (überschreibt das Plain-Text-Setting einmalig) |
| `Esc` | Popup schließen |
| `⌘B` / `Ctrl+B` | **Hintergrund freistellen** beim ausgewählten Image-Eintrag (ML — U²-Net) |
| `⌘S` / `Ctrl+S` | **Bild in Downloads speichern** (PNG unverändert) |

### Suchleisten-Befehle

Alle Power-Commands, generiert aus der kanonischen `CommandDoc`-Registry
(`core/frontend/src/lib/commandDocs.ts`). Tippe einen Befehl gefolgt von **`?`**
in die Suchleiste für die vollständige Inline-Hilfe (Argumente, Beispiele,
Tipps); **`?`** allein zeigt den Gesamtindex. (Die Hilfetexte selbst sind auf
Englisch.)

<!-- COMMANDS:START -->
<!-- Generated by scripts/gen-docs.mjs from core/frontend/src/lib/commandDocs.ts — do not edit by hand. -->

| Befehl | Seit | Was er tut |
|--------|------|------------|
| `rmvvls` | v0.18.0 | Vokale aus dem Text entfernen → Clipboard. |
| `tr` <sub>(alias: `tren`, `trde`, `trde2it`, `trit2de`, `trde2sp`, `trsp2de`, `trde2pl`, `trpl2de`)</sub> | v0.18.0 | Live-Übersetzung in der Preview — Enter kopiert, ⇧Enter öffnet Google Translate. |
| `kill` | v0.19.0 | Live-Prozess-Picker — nach Name/PID filtern, bestätigen, beenden. |
| `lock` | v0.19.0 | Bildschirm sofort sperren. |
| `mute` | v0.19.0 | System-Ausgabe stumm schalten (Toggle). |
| `reboot` | v0.19.0 | Rechner neu starten (mit Bestätigung). |
| `shutdown` | v0.19.0 | Rechner herunterfahren (mit Bestätigung). |
| `bruno` | v0.33.0 | Netto-Rechner Deutschland — Angestellte UND Selbständige (Steuerjahr 2025). |
| `freeze` | v0.35.0 | Eingabesperre — Tastatur + Maus bis zum Entsperr-Chord blockieren. |
| `pwgen` | v0.40.0 | Passwort-Generator — CSPRNG, 4 Modi. |
| `alarm` | v0.42.0 | Alarm zu einer Uhrzeit (nächstes Auftreten). |
| `timer` <sub>(alias: `countdown`)</sub> | v0.42.0 | Countdown-Timer — löst bei Ablauf einen Alarm aus. |
| `md2pdf` | v0.46.0 | Markdown → A4-PDF im mrxdown-Stil, neben der Quelle — überschreibt nie. |
| `wakelock` <sub>(alias: `caffeine`)</sub> | v0.52.0 | Mac wachhalten — voll (Bildschirm an) oder dark (Bildschirm darf schlafen). |
| `mkdir` | v0.53.0 | Ordner im vordersten Finder/Explorer-Ordner anlegen. |
| `terminal` | v0.53.0 | Terminal im vordersten Finder/Explorer-Ordner öffnen. |
| `touch` | v0.53.0 | Datei im vordersten Finder/Explorer-Ordner anlegen (opt. Inhalt). |
| `shot` <sub>(alias: `shotfull`, `shotwin`, `shotlast`)</sub> | v0.57.0 | Screenshot — Region / Vollbild / Fenster / Wiederholen, mit Selbstauslöser. |
| `clean` <sub>(alias: `cleanup`)</sub> | v0.60.0 | Speicher freimachen — Cache/Log/Temp + Dev-Müll, Ordner-Picker. |
| `brightness` <sub>(alias: `bri`)</sub> | v0.62.0 | Helligkeits-Slider pro Monitor im Preview. |
| `rnd` <sub>(alias: `random`)</sub> | v0.68.0 | Zufallszahl würfeln — in einem Status-Toast angezeigt. |
| `meme` | v0.70.0 | Meme-Ordner durchsuchen, gewähltes GIF/Bild kopieren. |
| `g` <sub>(alias: `ddg`, `gh`, `yt`, `npm`, `crates`, `so`, `wiki`)</sub> | v0.76.0 | Web-Such-Bangs — die Suche einer Seite für die Anfrage öffnen. |
| `hash` | v0.76.0 | Text als SHA-256 hashen → Clipboard (Hex). |
| `json` | v0.76.0 | Clipboard-JSON formatieren → Clipboard. |
| `jwt` | v0.76.0 | Clipboard-JWT dekodieren (Header + Payload) → Clipboard. |
| `qr` | v0.76.0 | QR-Code erzeugen — Live-Vorschau, PNG- und druckbarer STL-Export. |
| `slug` | v0.76.0 | Text sluggen (URL-sicher, klein, Bindestriche) → Clipboard. |
| `uuid` | v0.76.0 | Zufällige v4-UUID(s) erzeugen → Clipboard. |
| `sound` <sub>(alias: `audio`)</sub> | v0.80.0 | Audio-Ausgabe-Wähler + System-Lautstärke-Slider. |
| `trim` | v0.84.28 | Video/Audio-Datei trimmen — verlustfrei-schnell oder frame-genau. |
| `hue` | v0.84.40 | Philips-Hue-Lampensteuerung (lokal, nur LAN). |
| `disco` | v0.84.43 | Hue-Lampen zum Mikro beat-syncen — läuft nach dem Schließen weiter. |
| `stats` | v0.84.59 | Live-System-Dashboard — CPU/RAM/Akku/Sensoren/Disks/Netz + Verlauf. |
| `uptime` | v0.84.64 | Live-animierte Uptime-Anzeige. |
| `optim` <sub>(alias: `optimize`)</sub> | v0.84.71 | Ausgewählte Finder-Bild(er) komprimieren → Nachbardateien. |
| `rz` <sub>(alias: `resize`)</sub> | v0.84.72 | Ausgewählte Finder-Bild(er) skalieren (Lanczos3) → Nachbardateien. |
| `track` | v0.84.77 | Zeiterfassung — Start/Stopp, opt-in, verschlüsselt (macOS). |
| `boom` | v0.84.143 | Systemweiter Audio-EQ + Presets + Lautstärke-Boost. |
| `calendar` <sub>(alias: `cal`)</sub> | v0.84.234 | Monatskalender im Preview — welcher Wochentag war das Datum? |
| `snitch` | v0.84.246 | Netzwerk-Monitor + Best-Effort-Per-App-Blocker + Weltkarte (macOS). |
| `shazam` | v0.84.250 | Den laufenden Song über das Mikro erkennen. |
| `faker` <sub>(alias: `fake`)</sub> | v0.84.270 | Realistische Fake-Testdaten — 70+ Generatoren, 14 Locales, viele Formate. |
| `sec` <sub>(alias: `nmap`, `sqlmap`, `feroxbuster`, `ferox`, `john`)</sub> | v0.84.271 | Geführte Pentest-Command-Builder — nmap · sqlmap · ferox · John. |
| `figlet` <sub>(alias: `banner`, `ascii`)</sub> | v0.85.0 | ASCII-Art-Banner — Live-Vorschau, Hunderte Fonts durchblättern, Enter kopiert. |
| `settings` <sub>(alias: `config`)</sub> | v0.87.1 | Öffnet den Settings-Tab — optional direkt zu einer Sektion springen. |
| `weather` <sub>(alias: `wetter`)</sub> | v0.97.0 | Wetter für deinen Standort — aktuell, nächste 12 h + 5-Tage-Vorschau, animiert. |
| `tokens` | v0.101.0 | Claude-Code-Tokenverbrauch — Kosten, Projekte, Sessions & Modelle. |
| `iris` | v0.102.0 | Rotes Glimmen an den Bildschirmrändern, sobald das Mikrofon zu laut wird. |
| `loc` | v0.117.0 | Lines-of-Code-Statistik für die Finder-Auswahl — pro Sprache, mit Charts. |
| `adb` | v0.119.0 | Android-Handy steuern — Dashboard, Fernbedienung, Screenshot, Apps, WLAN-ADB. |
| `disk` <sub>(alias: `daisy`)</sub> | v0.120.0 | Speicher-Analyse à la DaisyDisk — Sunburst, was den Platz frisst. |
| `clock` | v0.121.0 | Weltzeituhr — Live-Zeiten der wichtigsten Städte. |
| `rickroll` | v0.122.0 | Du kennst die Regeln — und ich auch. |
| `repo` <sub>(alias: `export`)</sub> | v0.123.0 | Git-Repo-Statistik + Klonen — GitHub-URL einfügen: Zeiträume, Heatmap, Hotspots, Bus-Faktor. |
| `nosleep` | v0.124.0 | Mac am Netzteil dauerhaft wachhalten (pmset-Profil). |
| `alias` | v0.127.0 | Shell-Alias geführt anlegen — Befehl je OS + direkt anlegen. |
| `clown` | v0.132.0 | TeXt sO sChrEibEn — Galerie alberner Textstile. |
| `pagespeed` | v0.142.0 | Google PageSpeed Insights für eine URL — Desktop und Mobil nebeneinander. |
| `benchmark` <sub>(alias: `performance`)</sub> | v0.150.0 | CPU-Benchmark — Vorschau, bestätigen, dann Single- und Multi-Core-Werte. |
| `dezibel` <sub>(alias: `db`)</sub> | v0.154.0 | Live-Lautstärke vom Mikrofon in dB (SPL-Schätzung), animiert in der Vorschau. |
| `bluetooth` <sub>(alias: `bt`)</sub> | v0.159.0 | Bluetooth-Geräte verwalten — verbinden, trennen, entkoppeln. |
| `ip` | v0.172.0 | Öffentliche IP, ungefähren Standort, Netzwerk und Karte anzeigen. |
| `lumen` | v0.175.0 | Live-Umgebungshelligkeit in Lux, sofern das Gerät einen Sensor bereitstellt. |
| `btsniff` <sub>(alias: `btcap`, `btcapture`, `blecap`)</sub> | v0.179.0 | Bluetooth-Protokoll-Analyzer — Live-BLE-Scan + Captures importieren/dekodieren, Byte-Diff. |
| `mailcheck` <sub>(alias: `emailcheck`, `mailverify`, `mxcheck`)</sub> | v0.181.0 | E-Mail-Zustellbarkeit prüfen — Syntax, DNS/MX und eine nicht-invasive SMTP-Prüfung. |
| `convert` <sub>(alias: `cv`)</sub> | v0.186.0 | Einheiten & Währungen — `cv 165 mph` zeigt alle Geschwindigkeiten live, mit Einheiten-Autocomplete. |
| `8bit` <sub>(alias: `16bit`)</sub> | v0.190.0 | Live-Retro-Overlay — der ganze Bildschirm in 8- oder 16-Bit-Optik. |
| `gestures` <sub>(alias: `gesten`)</sub> | v0.190.0 | Gesten-Schutz: Live-Trackpad, Schwellen je Gerät, letzte Entscheidungen, Kalibrierung, Aufnahmen. |
| `limits` <sub>(alias: `usage`)</sub> | v0.190.0 | Nutzungslimits von Claude, Codex und Antigravity, mit Rücksetzzeit. |
| `task` <sub>(alias: `ki`)</sub> | v0.191.0 | KI-Tasks — beschreiben, die KI schreibt das Skript, du gibst frei, es läuft von selbst. |
| `speedtest` | v0.199.0 | Download, Upload, Latenz und Jitter messen — mit lokalem Verlauf. |
| `pulse` | v0.200.0 | Speicherdruck, Swap und SSD-Schreiblast — live und als Verlauf, mit Verursachern und Lebensdauer-Prognose (macOS). |
| `echo` | v0.201.0 | Text in eine Datei schreiben, wie in der Shell — Formatierung bleibt exakt. |
| `transcribe` | v0.205.0 | Audio/Video lokal → TXT mit Sprachwahl. |
| `spotify` | v0.206.0 | Spotify: läuft gerade, lokaler Verlauf, Export, Steuerung. |
| `github` | v0.209.0 | GitHub-Statistik: Follower, Sterne, Repository-Ranking und Wachstum über die Zeit. |

<!-- COMMANDS:END -->

### Komplette Feature-Matrix

| Feature | Wo triggern | Doku |
|---------|-------------|------|
| Clipboard-History (Text/RTF/HTML/PNG/Files, 1 000 Einträge, dedupliziert) | `Ctrl+Space` → suchen | core |
| **Rich-Copy-Treue** — kopiertes Markdown bleibt Markdown | Automatisch beim Erfassen | [clipboard-shapes.md](./docs/clipboard-shapes.md) |
| **Kopier-Formate + Lineage-Rails** — eine umgewandelte Kopie wird ein neuer Eintrag, per Commit-Graph-Pfad mit dem Original verbunden | Vorschau → `⌘`/`Ctrl` halten für die Transform-Chips | [clipboard-shapes.md](./docs/clipboard-shapes.md) |
| **Live-Übersetzung** — `tr*`-Commands übersetzen beim Tippen in der Vorschau | `tren <text>` tippen | [translation.md](./docs/translation.md) |
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
| Backup — komplette App als eine Datei (History + Snippets + Notes + 2FA + Settings, Timesheet optional), optional passwort-verschlüsselt | Settings → Backup & restore | [backup.md](./docs/backup.md) |
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
| **Finder-Selection-Actions** *(v0.30.0+, macOS)* | `Ctrl+Shift+F` | Popup listet die im Finder selektierten Dateien; `rz 1200x800` tippen skaliert alle Bilder (schreibt `<name>-1200x800.<ext>` neben Quelle), `optim` läuft oxipng auf jedes PNG. Enter auf einer Zeile öffnet die Datei |
| **Resize-Preset-Autocomplete** *(v0.31.0+)* | `rz` oder `rz <partial>` tippen | Beschriftete Preset-Zeilen (Full HD, HD, XGA, SVGA, …); Enter führt aus, Tab / → füllt ins Suchfeld vor dem Ausführen |
| **Screenshot-Vorschau-HUD** *(v0.32.0+)* | Nach `Ctrl+Shift+S` | CleanShot-X-Style schwebende Karte mit X / Pin / Copy / Save / Edit / Cloud Buttons über dem PNG. Pin behält die Vorschau über den nächsten Screenshot |
| **Annotations-Editor** *(v0.32.0+)* | Vorschau-HUD → Stift-Button | Neues Fenster mit 9 Tools: Pfeil / Linie / Text / Rect / Ellipse / Highlight / Blur (Mosaik-Pixelung) / Redact (deckender Block) / nummerierte Step-Badge. 4 Farb-Presets, 2–16 px Stroke, ⌘Z/⌘⇧Z undo/redo, ⌘S speichern, Esc abbrechen. Save backt zu `<App>-<ts>-edited.png` |
| **App-Name in Screenshot-Dateinamen** *(v0.32.0+)* | Automatisch | `osascript`-gefangener Frontmost-App-Name im gespeicherten Dateinamen: `Safari-20260524-153012.png`. Bearbeitete Varianten bekommen `-edited`-Suffix |
| Power-Command-Autocomplete (Fuzzy-Command-Matching) | Teil-Keyword tippen (`tre`, `rm`, `reb`, `bru`, `tim`, `pw`, …) → Vorschlag als `hint`-Zeile | core |
| **Markdown → PDF** *(v0.46.0+, macOS)* | `Ctrl+Shift+M` mit im Finder ausgewählten `.md`-Dateien | Automation → Finder |
| **2FA / TOTP-Manager** *(v0.47.0+)* | `2fa` oder `otp` tippen → Enter öffnet den TOTP-Tresor: Live-Codes + Countdown, **Hinzufügen / Bearbeiten (inkl. Secret) / Löschen, Drag-Umsortieren (⠿-Griff), Duplikate entfernen / Alle löschen**. **`2fa add [issuer]`** *(v0.104.0)* springt direkt ins Anlege-Formular (Issuer · Login · Base32-Secret), das Argument befüllt den Issuer vor — auch über einen dezenten ＋-Button in der Preview erreichbar. Import per Einfügen **oder Datei-Drag&Drop** (Google-Auth-Migration · Aegis · 2FAS · OTPManager · otpauth), dedupliziert beim Import. **Tippen filtert die Liste** — Fuzzy-Match über Issuer/Account, Top-Treffer umrandet, Enter kopiert dessen Code + schließt das Popup, Esc leert erst den Filter. `otp <issuer>` / `2fa <issuer>` vervollständigen einen Code inline. Secrets AES-verschlüsselt, überqueren nie IPC | core |
| **OTP-Autocomplete** *(v0.47.0+)* | `otp <Aussteller>` oder `2fa <Aussteller>` tippen (z. B. `2fa hosti` → Hostinger) → lebendiger 30-Sekunden-Countdown + Enter kopiert den aktuellen Code | core |
| **BPM-Detektor** | `bpm` tippen → Enter startet Live-Takterkennung via Mikrofon; **nochmal Enter pinnt** (Klick außerhalb schließt nicht mehr; Visualizer wird rot) | Mikrofon *(macOS)* |
| **Features-Tab** | History · Snippets · Notes · **Features** · Settings Tabs; Features-Tab listet alle Shortcuts und Fähigkeiten mit Live-Hotkey-Anzeige | core |
| **Overlay-Größen-Einstellung** | Settings → Appearance → Popup-Größe: Small / Medium / Large | core |
| **Status-Toast** *(v0.51.0+)* | Zentrierter Bildschirm-Toast bestätigt wakelock an/aus (und andere Zustandsänderungen) mit animiertem Ring | core |
| **Bildschirmaufnahme** *(v0.81.0+, macOS)* | `Ctrl+Shift+Alt+S` → Bereich → Audio (System / Mic / beides, Mic +10 dB) → 3-2-1 → MP4 (H.264) nach Downloads. Schwebende Stop-Leiste mit **Pause/Resume**. Multi-Monitor; System-Audio routet automatisch über ein BlackHole-Multi-Output und stellt danach zurück; `adeclick` + 256 k AAC für sauberen Ton. ffmpeg nötig | core |
| **Audio ersetzen / überlagern** *(v0.84.22+, macOS)* | `Ctrl+Shift+Alt+M` — Video im Finder wählen → Overlay zum **Ersetzen** oder **Mischen** einer lokalen Audiodatei oder eines **yt-dlp-YouTube-Tracks** an gewählter Startposition + Trim. Schreibt `-audioswap.mp4` daneben. ffmpeg (+ yt-dlp) nötig | core |
| **Social-Media-Download** *(v0.84.28+)* | **YouTube / Instagram / TikTok / Facebook**-URL einfügen/kopieren → in Suchleiste oder Clip auto-erkannt → Preview bietet **Video laden** (alle) + **Audio laden** (YouTube) → Downloads. Bevorzugt **H.264** (in QuickTime spielbar); bei YouTubes Bot-Schutz erneuter Versuch mit deinen Browser-Cookies (Chrome/Firefox/…). yt-dlp nötig | core |
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
- Sicherer Recursive-Descent-Parser in [`calc.ts`](./core/frontend/src/lib/calc.ts), kein `eval`. 43 Tests.

### Farb-Tools (v0.4.0 → v0.5.2)
- **Inline-Hex-Preview** — tippe `#3366FF` (auch `3366ff`, `#abc`, `#abcdef12`) → Swatch + Hex + RGB-Zeile oben → Enter pasted Großbuchstaben `#RRGGBB`.
- **HSV-Picker-Modal** — Hue-Slider, großes Swatch, Output-Tabs für Hex / RGB / HSL, Zwei-Klick-Auswahl (kein stiller Default), Copy via Tauri-Clipboard-Plugin (umgeht WKWebView-Restriktionen).
- **Pixel vom Bildschirm picken** — sample irgendein Pixel auf dem Desktop. macOS: Apples `NSColorSampler`-Lupe. Windows: Fullscreen-Overlay + `GetPixel`. Modul: [`screen_picker.rs`](./core/rust-lib/src/screen_picker.rs).
- Frontend in [`colors.ts`](./core/frontend/src/lib/colors.ts) + [`ColorPickerModal.tsx`](./core/frontend/src/components/ColorPickerModal.tsx). 37 Tests. Referenz: [`docs/colors.md`](./docs/colors.md).

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
Settings-Tab → *Backup & restore* → Export schreibt die komplette App (History, Snippets, Notes, 2FA-Konten, alle Settings — Timesheet optional) in eine Datei, optional passwort-verschlüsselt (AES-256-GCM + Argon2id). Import erkennt verschlüsselte Dateien, fragt das Passwort inline ab und merged zurück: Snippets upsert nach Abbreviation, History upsert nach SHA-256, 2FA/Timesheet dedupliziert, Settings überschreiben, Notes appended. Versioniertes Schema — neuere Backups werden abgelehnt statt still zu kappen. Referenz: [`docs/backup.md`](./docs/backup.md).

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
- **Footer** — zwei Zeilen. Oben: der Audio-Umschalter (wo der Ton gerade rauskommt — Lautsprecher, Bluetooth, Kopfhörer — bzw. `boom → <Gerät>` bei laufendem boom; Klick wählt ein anderes Ausgabegerät oder schaltet boom), die Sleep-/Wach- und Aufnahme-Anzeigen sowie die globalen Kürzel `⌃⇧O OCR` · `⌃⇧S Shot` · `⌃⇧C Color`, rechts der freie Speicherplatz. Unten: der Finder-Ordner, in dem die Datei-Befehle (`touch`, `echo`, `mkdir`, `terminal`, `daisy`) arbeiten, samt aktueller Auswahl — Klick zeigt ihn im Finder — und rechts die Version.
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

### Bruno — Brutto/Netto-Rechner (v0.33.0 · Selbständigen-Modus v0.86.0)
- **Befehl** — `bruno 60000` (jährlich) oder `bruno 5000m` (monatlich) im Suchfeld. Ergebnis-Zeile zeigt Netto/Monat + Netto/Jahr inline; Preview-Panel zeigt volle Aufteilung (KV / PV / RV / AV + ESt / Soli / Kirche + Abgabenquote + Grenzsteuersatz).
- **Freelancer / Selbständige (`f`-Suffix, v0.86.0)** — `bruno 80000f` rechnet das Netto aus dem Jahres**gewinn**; `bruno 7000mf` aus dem Monatsgewinn; `bruno 90000-15000f` aus Einnahmen − Betriebsausgaben. Modell: **freiwillige GKV** (14,0 % ermäßigt / 14,6 % mit Krankengeldanspruch + Zusatzbeitrag, auf den Gewinn zwischen Mindestbemessungsgrundlage und Beitragsbemessungsgrenze) **oder PKV-Fixbeitrag**; volle Pflegeversicherung; **keine RV-/AV-Pflicht**; Grund- oder **Splittingtarif**; **Gewerbesteuer** für Gewerbebetriebe (Freibetrag 24.500 €, Messzahl 3,5 % × Gemeinde-Hebesatz) inkl. **§ 35-EStG-Anrechnung** — Freiberufler bleiben befreit. USt ist durchlaufender Posten (nur § 19-Hinweis). Rechtsform, Hebesatz, GKV/PKV & Splitting unter **Settings → Bruno → Selbständig**.
- **Smart Defaults** — Steuerklasse I, NRW, 0 Kinder, kein Kirchen-Mitglied, TK-Niveau 2,45 % KV-Zusatz. Persönliche Werte via **Settings → Bruno** (in SQLite-Settings persistiert; `bruno-defaults-changed`-Event aktualisiert das Popup ohne Restart).
- **Steuerjahr 2025** — §32a EStG (vereinfacht), Grundfreibetrag 12.096 €, Beitragsbemessungsgrenzen KV 66.150 € / RV 96.600 €. Portiert aus der [Steuerschleuder](https://steuerschleuder.celox.io/)-Web-App des Maintainers.
- **Pure-TS-Compute** — kein IPC-Roundtrip pro Tastendruck. Zahlenformat-toleranter Parser (`bruno 60.000` = `bruno 60,000` = `bruno 60000`). 82 Unit-Tests pinnen Compute + Parser (beide Modi). ⚠️ Vereinfacht — kein Faktorverfahren, keine individuellen Freibeträge. Keine Steuerberatung.

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
│   ├── frontend/                      # React 19 · TS 5 · Tailwind v4 · Vite 7 — eine UI für alle 3 OSes
│   │   └── src/
│   │       ├── App.tsx                # Popup-Shell: kombinierte Liste, dispatchCommand, alle Inline-Panel-Verdrahtungen
│   │       ├── components/            # 57 Komponenten — SearchBar, HistoryList/Item, PreviewPanel, die Inline-Panels
│   │       │                          #   (Weather · Stats · Hue · Boom · Calendar · Shazam · Snitch · BPM · Equalizer …),
│   │       │                          #   die versteckten Games, der Screenshot-Editor, Settings/Features-Tabs
│   │       ├── hooks/                 # 8 Hooks — useClipboardHistory · useFuzzySearch · useSnippets · useKeyboardNav …
│   │       └── lib/                   # ~60 reine, unit-getestete Module — ipc.ts · commands.ts · commandDocs.ts · calc.ts
│   │                                  #   · convert.ts · bpm.ts · disco-engine.ts · weather.ts · figlet.ts · qr.ts …
│   └── rust-lib/                      # inspector-rust-core — GESAMTE Logik (66 Module + 10 Subsystem-Ordner)
│       ├── build.rs                   # linkt macOS Vision (OCR) + Metal (EDR-Helligkeit)
│       ├── models/u2netp.onnx         # U²-Net-Cutout-Model (~4,5 MB, Apache-2.0)
│       ├── assets/                    # eingebettete WAV-Cues + Alarm + ~550 gzip-Figlet-Fonts
│       └── src/
│           ├── lib.rs · commands.rs                 # Tauri-Builder + Tray + invoke_handler · ~290 #[tauri::command]s
│           ├── db.rs · models.rs · crypto.rs · settings.rs · ui_state.rs · backup.rs · sync.rs · logging.rs
│           │                                        #   SQLite (5 Tabellen) · Hash-Dedup + Prune · AES-256-GCM · Cloud-Sync (cue)
│           ├── clipboard_watcher.rs · snippets.rs · snippet_template.rs · notes.rs · seed.rs
│           │                                        #   Capture · Snippets + Templates + Notizen · First-Launch-Seed
│           ├── expander.rs · auto_expand.rs · paste.rs · hotkey.rs · input_lock.rs · esc_watch.rs · keepalive.rs
│           │                                        #   Text-Expander (4 Modi) · globale Hotkeys · Eingabesperre · Esc/Enter-Watcher
│           ├── text_field/                          # FieldAccess-Trait + macOS-AX + Windows-UIA In-Place-Replace
│           ├── region_picker.rs · ocr.rs · screen_record.rs · screenshot_preview.rs · screenshot_editor.rs
│           │                                        #   OCR · Screenshots (Region/Vollbild/Fenster) · Aufnahme · Preview-HUD · Annotate
│           ├── screen_picker.rs · color_loupe.rs    # Eyedropper + Live-Hex-Lupe
│           ├── social_dl.rs · media_trim.rs · audio_swap.rs · md_to_pdf.rs      # Medien: Download · Trim · Audio-Swap · md→PDF
│           ├── recolor.rs · cutout.rs · cutout_ml.rs · image_ops.rs             # Bild: Tint · Cutout (U²-Net) · Resize · Optim
│           ├── system_commands.rs · system_stats.rs · stats_history.rs · brightness.rs · edr.rs
│           │                                        #   kill/reboot/lock/mute · Live-Stats + Verlauf · Helligkeit + EDR/XDR
│           ├── audio.rs · sound.rs · mic_capture.rs · wakelock.rs              # Audio-Gerät · natives cpal-Mikro · Wachhalten
│           ├── boom/                                # systemweiter EQ — mod.rs (DSP) · macos.rs (Treiber-Bridge) · windows.rs (EqAPO)
│           ├── hue.rs · weather.rs · shazam.rs · snitch.rs · bruno.rs · timer.rs · alarm.rs · status_toast.rs
│           │                                        #   Inline-Panels & Integrationen (Hue · Wetter · Song-ID · Netz-Monitor …)
│           ├── gestures/ · window_snap/ · window_palette/    # Touchpad-Gesten + Fenster-Snapping/Palette (Per-OS)
│           ├── tracking/                            # Timesheet — os/ (aktives Fenster) · db · claude · bridge · slots · export
│           ├── faker/ · figlet/ · sec/              # Generatoren: Fake-Daten · ASCII-Banner · Pentest-Command-Builder
│           ├── totp_store.rs · totp_import.rs       # 2FA / TOTP-Tresor + Importer (GAuth · Aegis · 2FAS · OTPManager)
│           ├── translate.rs · cleaner.rs · meme.rs · app_launcher.rs           # Übersetzen · Disk-Cleaner · Meme-Picker · Launcher
│           ├── finder_selection.rs · frontmost_app.rs · osascript_util.rs      # Finder-Auswahl · touch/mkdir/terminal
│           └── cli_dispatch.rs · desktop_shortcuts.rs        # Linux-CLI-Flag-Dispatch + gsettings-Shortcut-Install
├── win/   ·   macos/   ·   linux/       # Per-OS-Bundle-Shells — 2-Zeilen-main.rs + tauri.conf.json + capabilities/ + icons/
│                                        #   (macos zusätzlich: entitlements.plist · linux: .desktop + Install-Docs)
├── boom-driver/                         # gevendorter BlackHole → „boom Audio"-Virtual-Driver (build.sh, ad-hoc-signiert)
├── extension/                           # MV3-Browser-Extension fürs Timesheet (meldet den aktiven Tab über einen Loopback-Socket)
├── memes/                               # Starter-Meme-Pack (Reaction-GIFs; auch die inspector-rust-memes.zip des Releases)
├── .github/workflows/                   # ci.yml (Rust- + Frontend-Tests) · release.yml (Bundles + GitHub-Release bei v*-Tags)
├── docs/                                # 19 Deep-Dive-Docs (Encryption · Timesheet · Figlet · Faker · Translation …)
│   ├── screenshots/                     #   die README-Screenshot-Galerie
│   └── *.png                            #   Brand-Artwork — ir.png · rust-juggernaut.png
├── scripts/                             # check.sh · install-{macos,linux}.sh · gen-docs.mjs · update-badges.mjs · gen-figlet-fonts.mjs …
├── Cargo.toml                           # Rust-Workspace — Members: core/rust-lib + {win,macos,linux}/src-tauri
├── pnpm-workspace.yaml                  # pnpm-Workspace (core/frontend + win/macos/linux)
└── package.json                         # Root-Scripts: dev/build:{win,macos,linux} · test · check · lint · typecheck · update-badges
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

### Tests & Coverage

Inspector Rust hält seine **pure Logik** — Parser, Mathematik, State-Machines, Arg-Builder, Formatierer — als freie Funktionen und testet sie erschöpfend (Verhalten, Edge-Cases, Fehlerpfade), während die unreine OS/FFI-Kante (CoreAudio/Vision/CGEvent-FFI, Tauri-Fenster, `ffmpeg`/`yt-dlp`/`osascript`-Spawns, Web Audio) manuell/per Integration getestet wird — sie braucht ein Live-System. Der deterministisch testbare Code ist daher gut abgedeckt — **Frontend `src/lib` ≈ 83 % stmt / 95 % Branch** (gemessen 2026-07-19) und die puren Rust-Kerne neben ihren 0 %-FFI-Shells (z. B. `window_snap/mod.rs` 93 %, `boom/mod.rs` 93 %) — auch wenn der Workspace-Schnitt bescheiden aussieht.

```bash
pnpm test               # Frontend-Unit-Tests (vitest + happy-dom) — 3396 Tests
cargo test --workspace  # Rust-Unit-Tests — 2155 Tests
```

Ein einzelnes Modul während der Iteration:

```bash
cargo test -p inspector-rust-core --lib snitch                              # ein Rust-Modul
pnpm --filter inspector-rust-frontend exec vitest run src/lib/worldmask.test.ts   # eine Frontend-Datei
```

Coverage-Reports (Tooling: `cargo-llvm-cov` + `@vitest/coverage-v8`):

```bash
cargo llvm-cov --lib -p inspector-rust-core --summary-only
cd core/frontend && npx vitest run --coverage --coverage.provider=v8 --coverage.include='src/lib/**'
```

Die Test-Commands laufen in [GitHub-Actions-CI](./.github/workflows/ci.yml) bei jedem Push und PR. Die **LOC- + Test-Count-Badges werden automatisch** aus den echten Quellen/Runnern berechnet — nie von Hand — via `pnpm update-badges` (`scripts/update-badges.mjs`, läuft auch als `posttest`-Hook; bricht ab, wenn eine Suite rot ist). Derselbe Lauf schreibt die Zahlen nach [`.github/repo-stats.json`](./.github/repo-stats.json), die die [Produktseite](https://inspector-rust.celox.io) liest; `repo-stats.test.ts` hält Datei und Badges deckungsgleich.

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
| **macOS "App ist beschädigt"** | Release-DMGs sind ad-hoc-signiert und **nicht notarisiert** (Notarisierung braucht eine kostenpflichtige Apple Developer ID). macOS setzt bei jedem Download ein Quarantäne-Attribut und meldet die App dann als **"beschädigt und kann nicht geöffnet werden"** — der Download ist in Ordnung; `codesign --verify --deep --strict` besteht auf dem ausgelieferten Bundle. **Rechtsklick → Öffnen hilft hier *nicht*.** Der Fix ist, das Quarantäne-Attribut einmalig zu entfernen, nachdem die App in den Programme-Ordner verschoben wurde: `xattr -dr com.apple.quarantine /Applications/InspectorRust.app` (oder **Fix Gatekeeper.command** auf dem DMG — das aus demselben Grund selbst blockiert sein kann, daher die Terminal-Zeile). Betrifft **jedes** Release, nicht nur alte. |
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


## Referenz-Dokumentation

Jede Referenzseite liegt in [`docs/`](./docs). Die Hilfe im Programm (`?`,
`<befehl>?`) und die Befehlstabelle oben werden aus derselben Registry
erzeugt; diese Seiten tragen das, was in keinen Tooltip passt —
Entwurfsentscheidungen, Fehlerbilder und die Fallen, die es gekostet hat.

| Seite | Inhalt |
|---|---|
| [reports.md](./docs/reports.md) | **Exported reports** — the one design system behind every HTML/PDF/PNG export, and the print traps |
| [inline-help.md](./docs/inline-help.md) | The `?` help system and the CommandDoc registry that feeds three surfaces |
| [transcribe.md](./docs/transcribe.md) | Local audio/video transcription with Whisper: setup, language and TXT output |
| [spec.md](./docs/spec.md) | The original product spec |
| [encryption.md](./docs/encryption.md) | At-rest encryption: what is encrypted, what deliberately is not |
| [backup.md](./docs/backup.md) | Full-app backup, encrypted backups, restore semantics |
| [snippets-import.md](./docs/snippets-import.md) | Snippet import/export formats and the merge rules |
| [text-expander.md](./docs/text-expander.md) | The four expansion modes and why each exists |
| [clipboard-shapes.md](./docs/clipboard-shapes.md) | Copy shapes, rich-text fidelity, lineage rails |
| [notes.md](./docs/notes.md) | Notes |
| [colors.md](./docs/colors.md) | Inline colour preview, picker, screen eyedropper |
| [timesheet.md](./docs/timesheet.md) | Time tracking: model, privacy, consolidated slots |
| [convert.md](./docs/convert.md) | `convert` / `cv` — units and currencies, input grammar, rate sources |
| [claude-limits.md](./docs/claude-limits.md) | `limits` / `quota` — Claude-Abo-Limits, Token-Umgang, Parser, Abfrage-Intervall |
| [ai-tasks.md](./docs/ai-tasks.md) | `task` / `ki` — KI-Tasks: Anbieter, Freigabe per Prüfsumme, Auslöser, Ausführung, Ideen |
| [8bit.md](./docs/8bit.md) | `8bit` / `16bit` — Live-Retro-Overlay: Modi, Einstellungen, Fokus-Modus, Lupe, Plattformgrenzen, Performance |
| [disk.md](./docs/disk.md) | `disk` / `daisy` — the usage sunburst and its blind spot |
| [repo.md](./docs/repo.md) | `repo` — git activity statistics |
| [tab-completion.md](./docs/tab-completion.md) | Tab-Vervollständigung der Befehlsparameter: Werte, Tippfehler-Korrektur, Gewohnheiten zuerst |
| [spotify.md](./docs/spotify.md) | `spotify` — lokaler Hörverlauf, Export und Steuerung der Spotify-App (macOS) |
| [github.md](./docs/github.md) | `github` — Follower, Sterne und Repo-Wachstum aus lokalen Snapshots; Anmeldung, Rate-Limits, Export |
| [pagespeed.md](./docs/pagespeed.md) | `pagespeed` — PageSpeed Insights, keys, IP restrictions |
| [pulse.md](./docs/pulse.md) | `pulse` — memory pressure, swap and SSD writes in the background; data model, retention, limits |
| [speedtest.md](./docs/speedtest.md) | `speedtest` — download, upload, latency against Cloudflare; method and limits |
| [adb.md](./docs/adb.md) | `adb` — Android device control |
| [faker.md](./docs/faker.md) | `faker` — realistic fake test data |
| [figlet.md](./docs/figlet.md) | `figlet` — ASCII banners and the bundled font set |
| [security-builder.md](./docs/security-builder.md) | `sec` — guided pentest command builders |
| [bluetooth-capture.md](./docs/bluetooth-capture.md) | `btsniff` — Bluetooth-Capture-Analyse (HCI/ATT/GATT-Dekodierung + Byte-Diff) |
| [mailcheck.md](./docs/mailcheck.md) | `mailcheck` — geschichtete E-Mail-Zustellbarkeit (Syntax · DNS/MX · nicht-invasives SMTP) |
| [cleanup.md](./docs/cleanup.md) | `clean` — the allowlist and the safety guarantees |
| [translation.md](./docs/translation.md) | `tr*` — the provider strategy |
| [ai-prompts.md](./docs/ai-prompts.md) | The bundled AI prompt snippets |
| [boom-driver-plan.md](./docs/boom-driver-plan.md) | `boom` — why a virtual audio driver was unavoidable |
| [macos-permissions.md](./docs/macos-permissions.md) | How the expander actually got working on macOS |
| [RELEASING.md](./docs/RELEASING.md) | Release process |

## 💸 Was es ersetzt

Inspector Rust bündelt in einer kostenlosen MIT-App, was sonst als Regal voller Einzel-Utilities verkauft wird. Das bekannte Pendant je Funktion, zu offiziellen Listenpreisen (Stand September 2026, gerundet — EU-Storefront-Preise inkl. MwSt. liegen bei diesen Apps erfahrungsgemäß etwa beim USD-Listenpreis; einige Anbieter preisen regional oder mit Dauer-Aktionen):

| Inspector-Rust-Funktion | Ersetzt | Preis (≈ €) |
|---|---|---|
| Clipboard-Verlauf (Pins, Notizen, Lineage) | [Paste](https://pasteapp.io/pricing) | 30 €/Jahr |
| Launcher, Befehlspalette, Rechner | [Alfred 5 Powerpack](https://www.alfredapp.com/shop/) | £34 ≈ 39 € |
| Text-Expander (Auto-Expansion, Snippets) | [TextExpander](https://textexpander.com/pricing) | 60 €/Jahr |
| Fenster-Snapping + Layout-Palette | [Moom](https://manytricks.com/moom/) | 15 € |
| Monitor-Helligkeit | [Lunar Pro](https://lunar.fyi/) | 23 € |
| EDR-Helligkeit über 100 % | [Vivid](https://getvivid.app/) | 10 € |
| System-EQ + Lautstärke-Boost (`boom`) | [Boom 3D](https://apps.apple.com/us/app/boom-3d-volume-booster-eq/id1233048948) | 50 € |
| Audio-Ausgabewahl + Routing (`sound`) | [SoundSource](https://rogueamoeba.com/soundsource/buy.php) | 49 € |
| Screenshots, Annotation, Pins, Screen-Recording | [CleanShot X](https://cleanshot.com/pricing) | 35 € |
| Bildschirm-OCR | [TextSniper](https://textsniper.app/) | 8 € |
| Farbpipette + Lupe | [Sip](https://sipapp.io/) | 30 € |
| System-Cleaner (`clean`)¹ | [CleanMyMac](https://macpaw.com/cleanmymac) | ~40 €/Jahr |
| Systemmonitor + Verlauf (`stats`) | [iStat Menus 7](https://apps.apple.com/us/app/istat-menus-7/id6499559693) | 12 € |
| Automatische Zeiterfassung (Timesheet) | [Timing Professional](https://timingapp.com/) | 108 €/Jahr |
| Video-Download (`social`) | [Downie](https://software.charliemonroe.net/downie/) | 20 € |
| Medien-Trim + Audio-Tausch | [Permute](https://software.charliemonroe.net/permute/) | 15 € |
| Trackpad-Gesten | [BetterTouchTool](https://folivora.ai/buy) | 15 € |
| Disk-Usage-Sunburst (`disk`) | [DaisyDisk](https://daisydiskapp.com/) | 10 € |
| Netzwerk-Monitor + Weltkarte (`snitch`)² | [Little Snitch 6](https://obdev.at/products/littlesnitch/order.html) | 59 € |
| Bluetooth-Geräte-Menü (`bt`) | [ToothFairy](https://apps.apple.com/us/app/toothfairy/id1191449274) | 7 € |
| Markdown → PDF | [Marked 2](https://apps.apple.com/us/app/marked-2-markdown-preview/id890031187) | 14 € |
| 2FA-/TOTP-Codes | [Step Two](https://apps.apple.com/us/app/step-two/id1448916662) (Unlock) | 10 € |

**Zusammen grob 420 € an Einmalkäufen plus rund 240 € jedes Jahr an Abos — Inspector Rust: 0 €.**

Dazu die Kategorien, in denen die gängigen Pendants ohnehin kostenlos sind (Amphetamine fürs Wachhalten, Shazam, Espanso, Maccy, …) — hier wohnen sie einfach in derselben Suchleiste.

¹ Der Cache-/Log-/Duplikate-Teil — CleanMyMacs Malware-/Uninstaller-Module haben hier kein Gegenstück.
² Monitor, Aktivität je App und die Live-Weltkarte — der Blocker ist bewusst best-effort; eine selbstsignierte App kann keine NetworkExtension-Firewall ausliefern, Little Snitchs Durchsetzungs-Schicht ist also ehrlich NICHT ersetzt.

Kein Anspruch auf Funktionsgleichheit — jede dieser Apps geht in ihrer Nische tiefer. Aber es sind die Werkzeuge, die diese App im Alltag ersetzt.

## License

[MIT](./LICENSE) — © 2026 Martin Pfeffer | [celox.io](https://celox.io)

A private open-source side project — built on weekends and evenings, made with ❤️.

Brewed and shipped from Berlin 🍻

---

## 🦾 Vom braven Detektiv zum Rust Juggernaut

Angefangen hat alles harmlos. Ein Hotkey, ein Clipboard-Manager und ein höflicher kleiner Detektiv mit Fedora, der dir einfach nur beim Einfügen helfen wollte.

Dann gaben wir ihm einen Text-Expander. Dann OCR. Dann Bildschirmaufnahme, einen systemweiten EQ, 2FA, eine Wettervorhersage, eine Netzwerk-Weltkarte, ASCII-Banner, eine Songerkennung, fünf versteckte Games und ungefähr neunzig weitere Befehle.

Er hat einfach weitertrainiert. Irgendwo zwischen dem U²-Net-Freisteller und dem EDR-Helligkeits-Overlay hörte er auf, Detektiv zu sein, und wurde eine *Kampfmaschine*. Der Hut blieb. Der Rest ist jetzt Hydraulik, vernietete Panzerung, ein festungstauglicher Security-Schild und eine unvernünftige Menge Ausrüstung.

Meine Damen und Herren — **Der Rust Juggernaut · Ultimate Armored Edition™.**
Immer noch ein Hotkey. Ungefähr 400 % mehr Panzerung. Immer noch unter 50 MB. 🕵️‍♂️ → 🦾

<div align="center">
  <img src="docs/rust-juggernaut.png?v=1" alt="Inspector Rust, verwandelt in den Rust Juggernaut — Ultimate Armored Edition" width="640" />
</div>
