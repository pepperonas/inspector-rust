# `record` — Bildschirmaufnahme + Demo-Video-Editor (OpenScreen-Umsetzung)

Stand: 2026-11-01 · Status: Entwurf zur Abnahme

## Absicht

Ein neuer Custom-Command `record` holt den Funktionsumfang von **OpenScreen v2.1.2**
(`getopenscreen/openscreen`, MIT, HEAD `bfd5bc32`, die gepflegte Fortführung des
archivierten `siddharthvaddem/openscreen`) in Inspector Rust: aufnehmen → im Editor
zu einem Demo-Video im Screen-Studio-Stil veredeln → als MP4/GIF exportieren.
Hauptanlass sind **Produkt-Demo-Videos** (z. B. für celox.io): das Ergebnis soll
nicht wie eine rohe Bildschirmaufnahme aussehen, sondern mit Zoom auf das
Geschehen, schönem Zeiger, Hintergrund, Rahmen und Untertiteln.

Der Nutzer will **alle Funktionen** von v2.1 — „1:1“ im *Verhalten* (gleiche
Zoom-Planung, gleiche Kurven und Konstanten), nicht im Code-Stack (kein Electron).

## Ziele

1. `record` (Custom-Command, roter Akzent, CommandDoc, Features-Tab, `features.txt`)
   öffnet den Aufnahme-Start; der Editor öffnet sich nach der Aufnahme automatisch.
2. **Ersetzt** den bestehenden ffmpeg-Bereichsrekorder (`screen_record.rs`,
   Overlay + Stop-Bar, Hotkey `Ctrl+Shift+Alt+S` = `ActionId` der Bildschirmaufnahme):
   der Hotkey startet künftig `record`. Der alte Code wird entfernt, sobald `record`
   die Aufnahme abdeckt.
3. Funktionsumfang v2.1 **außer dem KI-Chat-Assistenten** (siehe Inventar unten).
4. **Projekte sind nachbearbeitbar:** jede Aufnahme ist ein Projekt, das später
   erneut geöffnet, verändert und neu exportiert werden kann.
5. **MCP-Server + CLI** (headless `record` / `export` / Projekt-Werkzeuge).
6. UI in **Englisch + Deutsch** (der i18n-Katalog des Hauses) — nicht die 15
   Sprachen von OpenScreen.

## Nicht-Ziele

- **KI-Chat-Assistent** (BYO-Key Claude/OpenAI/…, schneidet per Chat) — ausdrücklich raus.
- Damit auch: **KI-geschriebene Shader-Effekte**. Die mitgelieferten Effekte bleiben
  wählbar; neue schreibt niemand per Chat. (Eigene WGSL-Effekte per Datei wären
  ein späterer Wunsch, nicht Teil dieses Designs.)
- **Freie Bereichsaufnahme** — fällt weg. Aufgenommen wird ganzer Bildschirm oder
  ein Fenster; zugeschnitten wird im Editor (genau wie OpenScreen).
- **KI-Übersetzung der Untertitel** (läuft in v2.1 über das Chat-LLM) — entfällt mit dem Chat.
- Weitere Sprachen außer EN/DE. Updater, „Star-Prompt“, Tray-Layout-Optionen
  von OpenScreen (Inspector Rust hat eigene Entsprechungen).

## Anti-Ziele (wäre trotz „funktioniert“ ein Fehlschlag)

- Vorschau und Export sehen **verschieden** aus.
- Auto-Zoom „pumpt“ (Jo-Jo) oder springt — das Verhalten muss die
  OpenScreen-Regeln haben (Gruppierung, Mindest-Halte- und Weit-Zeiten).
- Die 18 Wallpapers aus dem OpenScreen-Repo werden übernommen — ihre Herkunft
  und Lizenz ist nirgends angegeben. **Eigene** Hintergründe stattdessen.
- `record` verschlechtert die Popup-Latenz oder den Start der App (die schweren
  Teile — Compositor, ffmpeg, Whisper, ONNX — laden erst beim ersten Gebrauch).
- Ein Projekt geht verloren oder ist nach einem App-Update nicht mehr öffnbar
  (Projektformat versioniert, Migration statt Bruch).

## Plattform

**macOS zuerst.** Windows/Linux folgen später (wie bei `8bit`, `pulse`, `snitch`);
der Befehl trägt bis dahin `platform: ["mac"]`. OpenScreen deckt alle drei ab —
die Teile, die wir übernehmen, behalten ihre Windows/Linux-Pfade, werden aber
vorerst nicht gebaut oder getestet.

## Funktionsinventar (Referenz: `openscreen-src/new`, v2.1.2)

Die genaue Algorithmen-/Konstanten-Liste liegt im Analysebericht
(Scratchpad-Klon; beim Planen erneut aus dem Repo lesen, nicht aus dem
Gedächtnis). Kurzfassung dessen, was rein muss:

**Aufnahme**
- Display oder Fenster (ScreenCaptureKit; Apples Content-Sharing-Picker), 60 fps,
  max. 3840×2160, H.264 (Bitrate `clamp(w·h·fps·0.1, 2, 60 Mb/s)`, GOP 1 s).
- 3-2-1-Countdown, Pause/Fortsetzen, Speicherplatzprüfung, Displayschlaf verhindern,
  Notizen-/Teleprompter-Fenster.
- Systemton + Mikrofon (Gain je Gerät) → eine AAC-Spur 192 kb/s.
- Webcam als eigene Datei (30 fps).
- **Zeiger-Sidecar** (`*.cursor.json`): ~30 Hz Position (normiert), Klicks
  (Druck/Loslassen), Zeiger-Typ (16 Typen aus der AX-Rolle), Zeigerbild als
  Asset (SHA-256, einmal gespeichert), Sichtbarkeit; Modus „editierbares Overlay“
  (Systemzeiger ausgeblendet, später neu gezeichnet) oder „System“.

**Editor**
- Mehrere Clips (umordnen, duplizieren, einfügen), Spuren für Trim, Tempo
  (0,25–16×), Zoom, Annotationen, Layout (Kamera), Audio/Voiceover, Untertitel,
  Shader-Effekte.
- **Transkript-Schnitt** (Wörter löschen schneidet das Video).
- Einrasten (8 px), Tonwelle, Undo/Redo (50), konfigurierbare Tastenkürzel
  (Z/T/S/C/A/M/V …).
- Annotationen: Text (Schriften Inter/Lora/Oswald/Caveat/IBM Plex Mono, 6
  Animationen), Bild, Pfeil (8 Richtungen), Blur/Mosaik (Rechteck/Oval/frei).
- Zuschneiden je Clip mit Presets (frei, 16:9, 9:16, 1:1, 4:3, 3:4, 21:9).
- Voiceover aufnehmen, Musik/Audiospuren (Gain, Loop, Fades).

**Zoom**
- Auto-Zoom-Planer aus Klicks (Gruppierung ≤ 3 s, Fit-Test, Halte-/Weit-Zeiten,
  Konfliktauflösung ohne Jo-Jo), manuelle Zooms, 6 Tiefen (1,25–5×),
  Federkurve, Log-Skalen-Interpolation, Verkettung, Kamera folgt dem Zeiger mit
  Totzone, Klick-Pinning, 3D-Zoom-Presets (links/rechts/Orbit).

**Zeiger**
- Größe, Pfadglättung (Feder, 240 Hz Resampling), Klick-Bounce, Klick-Impact,
  Klick-Sound (Kenney, CC0), 3D-Zeiger, Zeiger-Bewegungsunschärfe,
  Auto-Hide, 7 Themes + eigene Zeiger.

**Bild**
- Wallpapers (eigene!) mit Bewegung (drift/aurora/waves), Verläufe, Volltonfarbe,
  eigenes Bild; Padding, Eckenradius, Schatten, Hintergrund-Blur, Tiefenschärfe,
  Bewegungsunschärfe; Gerätrahmen (Fenster, Laptop, Handy, Monitor; hell/dunkel);
  Ausgabe-Seitenverhältnis.

**Webcam**
- Layouts (PiP, Dual, vertikal, nur Kamera, ohne), PiP-Anker/Größe/Maske/
  Rundung/Spiegeln, reaktives Schrumpfen beim Zoom,
  **Hintergrund-Freistellung** (MediaPipe-Selfie-Segmentierung via ONNX).

**Untertitel**
- On-device, offline; Wortzeiten; Stil (Schrift, Größe, Platte, Position,
  Wörter je Zeile); Bearbeiten.

**Export**
- MP4 (H.264/HEVC/VP9; 720p/1080p/Quelle; 24/30/60 fps), GIF (eigener
  Encoder, 15–30 fps, Größen, Loop), True-Peak-Limiter −1,5 dBFS.

**Weiteres**
- Stil-Presets (`*.openscreenpreset`-Äquivalent), zuletzt geöffnete Projekte,
  Projektordner.
- **MCP-Server** (Projekte auflisten, Dokument-Werkzeuge, Medien, Transkription,
  Aufnahme) + **CLI** (`record`, `export`, Projektbefehle).

## Wie es gebaut wird (vom Nutzer entschieden)

- Ein neuer Custom-Command `record` in Inspector Rust (nicht eine separate App).
- Der alte Bereichsrekorder wird ersetzt.
- Nachbearbeitbare Projekte.
- EN/DE.
- MCP mit dabei.
- **Nur Apple Silicon:** auf Intel-Macs ist `record` ausgeblendet (wie `cutout`);
  der arm64-GitHub-Release enthält es inkl. LGPL-konform beigelegter
  ffmpeg-Dylibs. Der Intel-DMG baut ohne den Compositor.
- **Eigenes Projekt- und Preset-Format**, versioniert ab v1, angelehnt an den
  Aufbau von OpenScreens Schema v9 — **kein** Import von `.openscreen` /
  `.openscreenpreset`. „Migration statt Bruch“ gilt ab unserer v1.
- **MCP-Werkzeuge und CLI-Befehle nach Inspector-Rust-Konventionen** benannt
  (gleicher Funktionsumfang, keine Namenskompatibilität zu OpenScreen nötig).

## Wie es gebaut wird (Entscheidung des Assistenten — prüfbar)

0. **Der Compositor läuft in einem eigenen Hilfsprozess** (`ir-record-render`,
   Muster wie `ir-ocr`): ffmpeg/Metal/ONNX belasten nie den Start der Tray-App,
   ein Absturz dort reißt nicht Zwischenablage-Verlauf & Co. mit, und MCP/CLI
   können denselben Helfer nutzen. Vorschau-Frames und Export-Fortschritt
   kommen über eine definierte Schnittstelle zurück (Details: Planung).

1. **Den OpenScreen-Rust-Compositor übernehmen statt neu schreiben.** v2.1
   rendert Vorschau *und* Export in `crates/compositor` (MIT, ~72k Zeilen,
   macOS-Pfad: Metal + VideoToolbox + MSL-Shader, plus wgpu/naga für Shader-
   Effekte). Er wird als eigene Workspace-Crate vendort (Herkunft + Commit im
   Kopf, Lizenz in den Third-Party-Hinweisen) und über eine schmale Rust-API
   angesprochen. Begründung: „1:1“ im Verhalten heißt dieselben Formeln; die
   stehen dort, getestet. Vorschau und Export laufen durch denselben Code
   (erfüllt das wichtigste Anti-Ziel per Konstruktion).
2. **Electron-/TypeScript-Logik wird portiert, nicht eingebettet:** Projekt-
   dokument (Zod-Schema v9 → Rust-`serde` + TS-Typen), Zoom-Planer, Zeitleisten-
   Operationen, Tastenkürzel, Annotationen. Reine Logik wandert nach Rust
   (testbar, auch für MCP/CLI nutzbar); die Editor-Oberfläche ist React im
   Haus-Stil (`lib/` + Komponenten, MD-Tokens, kein Overshoot — siehe
   `.claude/rules/animation.md`).
3. **Aufnahme mit Inspector Rusts eigenem ScreenCaptureKit-Code** (`objc2-*`,
   bereits für `8bit`/`16bit` im Einsatz) + AVAssetWriter, statt der Swift-
   Helfer von OpenScreen. Zeigerdaten über einen Listen-only-`CGEventTap`
   (Hausmuster) + AX-Rolle für den Zeiger-Typ + `NSCursor.currentSystem` für
   das Bild.
4. **Der Editor ist ein eigenes Tauri-Fenster** (wie der Screenshot-Editor),
   die Live-Vorschau des Compositors wird darin eingebettet (Metal-Layer unter
   bzw. neben dem Webview — genauer Weg ist Sache der Planung).
5. **Untertitel mit whisper.cpp** wie OpenScreen (Modell `ggml-small-q8_0`,
   Silero-VAD), beim ersten Gebrauch heruntergeladen — gibt Wortzeiten und
   braucht keinen Python-venv. (Der `transcribe`-Python-Whisper bleibt davon unberührt.)
6. **Projektordner** `~/Movies/Inspector Rust/` (ein Unterordner je Projekt:
   Videos, Zeiger-Sidecar, Projekt-JSON); Exporte nach `~/Downloads`. In den
   Settings änderbar.
7. **Eigene Wallpapers** erzeugen (prozedural/selbst gerendert), da die
   OpenScreen-Bilder keine geklärte Lizenz haben.
8. **CLI** über Inspector Rusts vorhandene Kommandozeilen-Weiterleitung
   (`cli_dispatch.rs`, Single-Instance); **MCP-Server** als stdio-Server
   (separates Binary oder Modus der App — Sache der Planung).

## Bekannte Risiken / Randbedingungen

- **FFmpeg-Linking:** der Compositor bindet auf macOS die FFmpeg-Bibliotheken
  (LGPL, *shared*) per bindgen. Inspector Rust ruft ffmpeg heute nur als
  externes Programm auf. Die Dylibs müssen ins App-Bundle (vor dem Signieren,
  wie der boom-Treiber) und LGPL-konform beiliegen. Bundle wächst deutlich.
- **ONNX Runtime doppelt:** Inspector Rust linkt `ort` statisch (`cutout`),
  der Compositor `ort` mit `load-dynamic`. Versionen/Features müssen
  zusammenpassen; ggf. Segmentierung auf das vorhandene `ort` umstellen.
- **Zwei Metal-Bindings** (`metal` 0.29 / `objc` 0.2 im Compositor vs.
  `objc2-metal` im Haus) — koexistieren, aber Speicher-/Größenkosten.
- **Build-Zeit + Binärgröße** steigen spürbar (wgpu, naga, cosmic-text,
  Compositor). Muss gemessen und im Commit genannt werden.
- **Berechtigungen:** Screen Recording, Accessibility (Zeiger-Tap),
  Mikrofon, Kamera — Kamera ist neu für die App (Info.plist-Usage-String).
- **Umfang:** mehrere Wochen Arbeit. Es wird in Etappen geliefert, jede für
  sich benutzbar.

## Vorgeschlagene Etappen (jede einzeln lieferbar)

1. **Aufnahme + Projekt:** `record`-Befehl, Display/Fenster, Countdown, Pause,
   Ton, Zeiger-Sidecar, Projektordner, Hotkey-Umzug, alter Rekorder raus.
   Ergebnis: Rohvideo + Zeigerdaten + Projekt-JSON.
2. **Compositor + Editor-Grundgerüst + Export:** vendorte Crate gebaut,
   Editor-Fenster mit Live-Vorschau, Zeitleiste mit Trim/Tempo/Crop, MP4/GIF.
3. **Zoom + Zeiger + Bild:** Auto-Zoom-Planer, manuelle Zooms, Zeiger-Rendering,
   Hintergründe, Rahmen, Schatten, Bewegungsunschärfe.
4. **Annotationen, Audio, Tastenkürzel, Presets, Projekte wieder öffnen.**
5. **Webcam** inkl. Layouts und Freistellung.
6. **Untertitel + Transkript-Schnitt.**
7. **3D-Zoom, 3D-Rahmen, 3D-Zeiger, Shader-Effekte.**
8. **MCP-Server + CLI.**

**Planung:** Etappe 1 wird ausführbar im Detail geplant; die Etappen 2–8
bleiben Gliederung und werden je für sich geplant, sobald die vorige steht
(Etappe 2 klärt erst, was mit dem Compositor praktisch geht).

## Dem Bauenden überlassen

- Genaue API-Grenze zum Compositor; wie die Live-Vorschau ins Tauri-Fenster kommt.
- Wie viel des TS-Editors als UI übernommen vs. neu im Haus-Stil gebaut wird
  (Verhalten muss gleich sein, Optik folgt Inspector Rust).
- Aufteilung der IPC-Befehle, Events, Settings-Schlüssel.
- Ob MCP ein eigenes Binary oder ein App-Modus ist (der Render-Helfer ist
  ohnehin eigenständig).
- Testaufbau (Hausregel: reine Logik extrahieren und testen, Mutationsprobe für
  neue Pins; die Compositor-eigenen Tests mitnehmen).
