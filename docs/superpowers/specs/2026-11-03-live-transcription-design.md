# Live-Transkription mit Ticker-Overlay — Design

Stand 2026-11-03. Freigegeben im Chat ("engine ersetzen"). Branch
`feat/live-transcribe`, Worktree `../inspector-rust-live-transcribe` (eine
zweite Session arbeitet parallel auf `main` an der Bildschirmaufnahme).

## Absicht

Gespräche (Meetings, Calls, Videos) lokal mitschreiben: **Mikrofon (ich) und
Systemton (Gegenüber) gleichzeitig**, getrennt beschriftet, im Hintergrund,
mit einem kleinen Laufband-Overlay und Transkriptdateien, die einen Absturz
überleben. Keine Cloud. Deutsch und Englisch müssen sehr gut funktionieren.

Gleichzeitig ersetzt die neue Engine die bisherige Python/openai-whisper-
Transkription (Diktat `transcribe --start/--song/--room`, Datei-Modus
`transcribe <pfad>`). Danach braucht die App kein Python-venv mehr.

## Ziele

- MIC und SYS gleichzeitig, getrennt transkribiert, jede Zeile mit Quelle.
- Ticker: Text höchstens ca. 3 s hinter dem Gesprochenen, flüssig, klick-durchlässig.
- Stabil ≥ 2 h im Hintergrund ohne sichtbares Hauptfenster, ohne Speicherleck.
- `kill -9`: höchstens das laufende Segment fehlt; nächster Start erkennt die
  Session als abgebrochen und meldet sie.
- Lautsprecherbetrieb erzeugt keine doppelten Zeilen (MIC-Echo des SYS-Tons).
- Deutsch und Englisch, auch gemischt in einer Session.
- Bestehende `transcribe`-Befehle behalten ihre Bedienung, laufen aber auf der
  neuen Engine.

## Nicht-Ziele

- Keine Cloud-Transkription, keine Sprecher-Diarisierung innerhalb einer Quelle.
- Keine anderen Sprachen als de/en im Auto-Modus (Whisper könnte mehr; die
  Erkennung wird bewusst auf de/en beschränkt, s. unten).
- **Transkription nur noch auf Apple-Silicon-Macs** (Nutzer-Entscheid
  2026-11-03). Windows, Linux und Intel-Macs verlieren die (dort ungetestete)
  Python-Datei-Transkription; die `transcribe`-Befehle bekommen dort
  `platform: ["mac"]` + Laufzeit-Check auf arm64, das Intel-DMG bekommt keinen
  Helper (wie record-render). CI baut whisper.cpp nur auf dem macOS-Runner.

## Anti-Ziele (wäre ein Fehlschlag, auch wenn es "funktioniert")

- Eine Aufnahme, die man nicht sieht (Datenschutz): Aufnahme muss jederzeit im
  Tray erkennbar sein.
- Stilles Weiterlaufen bei Schreibfehlern (Datenverlust ohne Warnung).
- Ein Ticker, der flackert/springt oder Klicks schluckt.
- Ein Absturz der Inferenz (ggml ruft bei Assertions `abort()`), der die
  wochenlang laufende Tray-App mitreißt.
- Die laufende Aufnahme-Session (Bildschirmaufnahme) beeinträchtigen.

## Architektur

### Crates

- **`transcribe-core`** (neu, Workspace-Member, keine Tauri-Abhängigkeit):
  Resampling, VAD, Segmentierung, LocalAgreement, Dedupe, Writer/Persistenz,
  Session-Recovery, Modellverwaltung (Download + SHA), Whisper-Inferenz
  (whisper-rs, Feature `metal`), Audioquellen (cpal-Mikrofon; macOS Process Tap
  + ScreenCaptureKit-Fallback hinter `cfg(target_os = "macos")`). Wiederverwendbar
  für eine spätere Diktier-App.
- **`ir-transcribe`** (neues Binary-Crate `transcribe-helper/`): dünner Prozess
  über `transcribe-core`, Protokoll JSON-Zeilen über stdin/stdout. Muster wie
  `ir-ocr`/`ir-record-render`: liegt in `Contents/MacOS/`, wird vor dem Signieren
  kopiert.
- **`inspector-rust-core`** (Tauri): startet/überwacht den Helper, übersetzt in
  Commands/Events, Tray, Hotkeys, Ticker-Fenster, Settings, Recovery-Hinweis.

**Meine Entscheidung (nicht vom Nutzer):** Inferenz + Aufnahme + Writer im
Helper-Prozess, nicht in der App. Gründe: (1) ggml/whisper.cpp bricht bei
internen Fehlern per `abort()` ab — im App-Prozess beendet das die Tray-App;
(2) das Modell belegt mit Metal ~1–2 GB, die nach dem Stopp durch Prozessende
garantiert frei sind (die App läuft wochenlang); (3) whisper.cpp/ggml landet
nicht im App-Binary. TCC-Berechtigungen (Mikrofon, Audio Capture) gelten für
den verantwortlichen Prozess (die App-Bundle), wie bei `ir-ocr`. Stirbt die
App (`kill -9`), sieht der Helper EOF auf stdin, schließt die Session sauber
ab und beendet sich; finale Segmente sind dann schon auf Platte.

### Audioquellen

- **MIC:** cpal, Standard-Eingabegerät oder in Settings gewähltes Gerät.
- **SYS:** Core Audio Process Tap (macOS 14.2+; `AudioHardwareCreateProcessTap`
  mit globalem Tap, privates Aggregate-Device, IOProc). Gewählt, weil: keine
  Bildschirmaufnahme-Berechtigung (nur `NSAudioCaptureUsageDescription`), kein
  Videostream, kein lila Bildschirmfreigabe-Indikator, und er erfasst den Ton
  auch dann, wenn boom Audio die Standardausgabe ist. Fallback: das vorhandene
  ScreenCaptureKit-Audio (`system_audio.rs` wandert in den Kern) bei < 14.2
  oder verweigertem Tap. Der Rechner läuft macOS 27, der Tap ist also der
  Normalfall. Hinweis aus der boom-Historie: ein Tap ist ein reiner
  Aufnahme-Weg — genau das wird hier gebraucht; nichts wird stummgeschaltet.
- Beide Streams → 16 kHz mono f32 (rubato). Getrennt, kein Mixing.
- Jede Quelle einzeln in Settings und Tray schaltbar.

### VAD und Segmentierung (pro Stream)

- Silero VAD v5 über `ort` (bereits Abhängigkeit; ONNX-Modell ~2 MB
  eingebettet wie `u2netp.onnx`). Eigene Instanz pro Stream.
- Segment: Vor-/Nachlauf je 300 ms, max. 15 s, dann erzwungener Schnitt an der
  leisesten Stelle der letzten Sekunden (Muster aus dem alten `Segmenter`).
- Ticker-Hypothesen: gleitendes Fenster über das offene Segment (max. letzte
  5 s Kontext beim Anzeigen, Decode des ganzen offenen Segments), alle ~1 s.
  **LocalAgreement-2:** der längste gemeinsame Wortpräfix zweier aufeinander-
  folgender Hypothesen gilt als stabil (im Ticker hell), der Rest vorläufig.
  Endet das Segment (VAD-Stille/Max-Länge), wird es final decodiert; der finale
  Text ersetzt die Hypothese und geht in die Datei.
- Bewährte Regeln aus dem Python-Worker ziehen um: Konfidenzfilter
  (`avg_logprob ≥ −1.0`, Kompression ≤ 2.4 bzw. whisper.cpp-Äquivalente),
  Schleifenfilter (≥ 3 identische Zeilen), Rauschtexte (`is_noise_text`).

### Transkription

- whisper-rs mit Metal, Standard `ggml-large-v3-turbo-q5_0` (~550 MB), in
  Settings wählbar (small, medium, large-v3-turbo q5/q8). Ein `WhisperContext`,
  je Stream ein `WhisperState`.
- Inferenz-Queue mit Priorität: finale Segmente vor Hypothesen; eine Hypothese
  wird verworfen, wenn für denselben Stream eine neuere ansteht. Ein Worker-
  Thread (GPU-Last kontrolliert).
- **Sprache:** Setting Auto / Deutsch / Englisch. Auto = whisper-Spracherkennung
  über die Wahrscheinlichkeiten, aber **nur de vs. en verglichen** (der Rest
  ignoriert) — verhindert Fehlerkennung kurzer Segmente (dokumentiert: Shona).
  Pro Segment neu, damit Sprachwechsel funktionieren; für Hypothesen gilt die
  Sprache des letzten finalen Segments der Quelle.
- Prompt: Ende des bisherigen finalen Texts der Quelle (max. ~200 Zeichen), wie
  bisher.
- Modelle unter `~/Library/Application Support/InspectorRust/models/`, Download
  mit Fortschritt (Event), SHA-256-Prüfung gegen eine im Code feste Tabelle,
  Schreiben als `.part` + rename.

### Echo-Dedupe

MIC-Segment wird verworfen, wenn es mit einem SYS-Segment zeitlich überlappt
(Toleranz ±1 s) und die normalisierte Levenshtein-Ähnlichkeit ≥ Schwelle
(Standard 0.75, Setting) ist. MIC-Finals warten dafür kurz (bis das zeitgleiche
SYS-Segment final ist, max. 3 s), bevor sie geschrieben werden. Kopfhörer-
Erkennung (Ausgabegerät mit Transport Bluetooth/kopfhörer-artig, bzw. Datenquelle
"Headphones") schaltet Dedupe automatisch ab (Setting "automatisch / immer / nie").

### Persistenz

- Writer-Thread, Channel, Pipeline blockiert nie auf I/O.
- Session-Ordner `~/Documents/Transkripte/YYYY-MM-DD_HHMMSS/` (Setting):
  `transkript.txt` (`[19:45:12] MIC: …`), `transkript.jsonl` (start/end ms,
  source, lang, confidence, text), `session.json` (start, model, sources,
  status `running|completed|aborted`, end).
- Append-Modus, nach jedem Segment `flush` + `sync_data`. `session.json` wird
  atomar ersetzt (tmp+rename) — die einzige nicht-append Datei.
- Hypothesen nie in Dateien.
- Rohaudio optional (Standard an): pro finalem VAD-Segment eine kleine WAV
  (16 kHz mono i16) unter `audio/<seq>-<src>.wav`, vollständig geschrieben
  (Header korrekt). Aufbewahrung in Tagen (Standard 7), Aufräumen beim Start.
- Start-Recovery: Sessions mit `running` → `aborted` + Ende = letzter Eintrag
  der jsonl; Nutzer per Status-Toast informieren.
- Schreibfehler: Wechsel auf Fallback-Ordner
  `~/Library/Application Support/InspectorRust/Transkripte/…` (neuer Session-
  Ordner, Vermerk in `session.json`), Warnung im Tray und als Toast. Scheitert
  auch der Fallback: Aufnahme stoppen und melden — nie still weiterlaufen.

### Hintergrundbetrieb

- Steuerung über Tray und globale Hotkeys (`ActionId`-Registry, belegbar):
  Start/Stopp und Ticker ein/aus. Defaults so wählen, dass keine Kollision mit
  bestehenden Bindings (Kollisionsprüfung `live_bindings`).
- App Nap: `NSProcessInfo beginActivityWithOptions(UserInitiated)` im Helper
  während der Aufnahme.
- Pipeline unabhängig vom WebView.
- Tray: Aufnahme-Indikator (Icon-Variante mit rotem Punkt, Menüeintrag
  "Transkription läuft — stoppen", Quellen-Haken, Ticker-Haken).

### Ticker-Overlay

- Eigenes Fenster `transcribe-ticker`: transparent, rahmenlos, always on top,
  nicht fokussierbar, `set_ignore_cursor_events(true)`, `setCanHide:NO`,
  Level über Vollbild, Collection Behavior canJoinAllSpaces |
  fullScreenAuxiliary | stationary; gezeigt mit `orderFrontRegardless`
  (Hausregel, sonst Fokus-Diebstahl).
- Leiste oben/unten, Bildschirm wählbar, Breite, Schriftgröße, Deckkraft,
  Geschwindigkeit einstellbar.
- Text läuft rechts→links; final hell, vorläufig grau/kursiv; Vorläufiges wird
  beim Finalisieren ersetzt ohne Sprung (Spans mit stabiler Breite-Ermittlung,
  Bewegung nur per `transform` in einer rAF-Schleife).
- MIC/SYS je Akzentfarbe + kleines Label.
- Unabhängig von der Aufnahme ein-/ausblendbar.

### Berechtigungen

- `NSMicrophoneUsageDescription` (vorhanden), `NSAudioCaptureUsageDescription`
  neu (Injection in `install-macos.sh` und `pack-macos-release-dmg.sh`).
- Prüfung beim Start mit Hinweis + Direktlink in die Systemeinstellungen.

### UI im Hauptfenster

- Befehl/Panel "Transkription" (Live-Konferenz-Modus): Start/Stop,
  Quellenwahl, Live-Ansicht, Liste vergangener Sessions (Finder öffnen,
  abgebrochene markiert).
- Settings-Sektion: Modell (+ Download-Status), Sprache, Geräte, Speicherpfad,
  Rohaudio + Aufbewahrung, Dedupe-Schwelle + Kopfhörer-Modus, Ticker-Optik,
  Hotkeys (Verweis auf Global shortcuts). Footer im Settings-Tab:
  `© {Jahr} Martin Pfeffer | celox.io` — prüfen, ob schon vorhanden.
- Alle Texte en+de im Katalog.

### Migration der alten Befehle

- `transcribe --start/--stop/--song/--room`: weiter eine Quelle, Text in die
  Zwischenablage — jetzt über den Helper; der Genauigkeits-Endpass entfällt
  (turbo ist bereits das genaue Modell) oder läuft optional mit demselben Modell
  über die ganze Aufnahme. Shazam-Teil (Songtext) bleibt unverändert in der App.
- `transcribe <pfad>`: Datei → ffmpeg? Nein: Decodierung über `symphonia`
  oder den vorhandenen AVFoundation-Weg (Entscheidung im Plan nach Recon), dann
  dieselbe Engine; TXT neben die Datei wie bisher.
- Python-Assets (`runner.py`, `live.py`, `scripts/setup-transcribe.py`) und der
  venv-Check entfallen; Doku (`docs/transcribe.md`, CLAUDE.md, CommandDocs,
  features.txt, CHANGELOG) wird angepasst.

## Constraints

- Hausregeln aus CLAUDE.md (i18n en/de, Doku-Vertrag, Bundle-Budget, keine
  sync Commands, die blockieren, `confirmDialog` statt `window.confirm`, Tests
  für die pure Logik, Mutationsproben).
- Plattenplatz: auf dem Rechner sind ~5 GB frei; ein zweites `target/` braucht
  ~8 GB plus Modell. Muss vor dem ersten Build geklärt werden.
- `cmake` fehlt (whisper-rs braucht es) → `brew install cmake`.

## Entscheidungen nach dem Review (meine Calls, nicht vom Nutzer)

- **Parallele Session:** eigene Skript-Datei `scripts/bundle-transcribe.sh`
  statt Edits in `bundle-record-render.sh`; Änderungen an `install-macos.sh`,
  `pack-macos-release-dmg.sh`, `lib.rs`, `commands.rs`, `Cargo.toml` auf
  wenige, klar abgegrenzte Einfügungen beschränkt. Vor dem Merge auf den dann
  aktuellen `main` rebasen; gemerged wird, wenn die Aufnahme-Session ihren Stand
  committet hat. Kein geteiltes `CARGO_TARGET_DIR`.
- **Modell:** Download beim ersten Bedarf nach expliziter Bestätigung im Panel
  bzw. in Settings (Größe wird genannt), mit Fortschritt. Offline/kein Modell:
  klare Meldung, keine Aufnahme ohne Modell.
- **Gleichzeitigkeit:** ein Helper, eine Session zur Zeit (Konferenz-Live,
  Diktat, Song/Raum, Datei schließen sich gegenseitig aus — wie heute der
  Busy-Flag). Kein doppeltes Laden des Modells.
- **Song-Modus:** nutzt künftig ebenfalls den Process Tap (Fallback SCK);
  das opt-in Ducking bleibt. Shazam-Erkennung bleibt in der App, der Helper
  liefert ihr die Fenster.
- **Genauigkeits-Endpass entfällt:** jedes Segment wird bereits mit dem
  eingestellten (genauen) Modell final decodiert; der Text ist der zusammen-
  gesetzte Finaltext. Spart das Halten der ganzen Aufnahme im Speicher.

## Dem Bauenden überlassen

Konkrete Datei-/Modulschnitte, Protokollformat im Detail, Hotkey-Defaults
(kollisionsfrei), Ticker-Animation im Detail, Audio-Decoding im Datei-Modus,
genaue Modell-URLs/SHA (Hugging Face `ggerganov/whisper.cpp`).
