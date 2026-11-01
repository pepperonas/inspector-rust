# `record` Etappe 1 — Aufnahme + Projekt — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `record` nimmt einen Bildschirm oder ein Fenster (ScreenCaptureKit) mit Systemton, Mikrofon und Zeiger-Sidecar als nachbearbeitbares Projekt in `~/Movies/Inspector Rust/` auf und ersetzt den alten ffmpeg-Bereichsrekorder vollständig.

**Architecture:** Neues Rust-Modul `core/rust-lib/src/record/` — reiner Kern (Größen, Bitrate, Zeitstempel-Gate, Pausenuhr, Zeiger-Sichtbarkeit, Projektformat, WAV) unit-getestet; dünne macOS-Ränder: SCStream → AVAssetWriter (Video + Systemton), cpal-Mikrofon → WAV, Zeiger-Sampler (CGEventTap + AX + NSCursor), Quellwahl über `SCContentSharingPicker`. Drei kleine Fenster (HUD, Countdown, Steuerleiste) + ein Vorschau-Panel für den Befehl. Kein Editor in dieser Etappe: nach dem Stopp wird der Projektordner im Finder gezeigt.

**Tech Stack:** Rust (objc2 0.6, objc2-screen-capture-kit 0.3.2, **neu: objc2-av-foundation 0.3.2**, objc2-core-media/-core-video, cpal), Tauri 2, React 19 + TS, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (Etappe 1). Referenzquelle (nur lesen): OpenScreen v2.1.2 `getopenscreen/openscreen@bfd5bc32`, v. a. `electron/native/screencapturekit/Sources/**` und `src/native/contracts.ts`.

## Global Constraints

- Nur macOS, nur Apple Silicon: `CommandSpec.platform = ["mac"]`; Rust-Code unter `#[cfg(target_os = "macos")]`; auf `x86_64-apple-darwin` meldet `record_supported()` false (Intel-DMG baut weiter).
- Laufzeit-Gate: `record` braucht `SCContentSharingPicker` (macOS 14+); fehlt die Klasse → Panel zeigt „benötigt macOS 14“, keine Aufnahme.
- Video: max 3840×2160, 60 fps, H.264, Pixelformat 420YpCbCr8BiPlanarVideoRange, Bitrate `clamp(w·h·fps·0.1, 2_000_000, 60_000_000)`, Keyframe-Intervall = fps Frames (1 s), `AllowFrameReordering = false`.
- Systemton: 48 kHz stereo, `excludesCurrentProcessAudio = true`, AAC 192 kb/s in derselben MP4.
- Mikrofon: **eigene Datei** `mic.wav` (Entscheidung: später im Editor pegelbar; Abweichung von OpenScreens Mischung zur Aufnahmezeit, im Export unsichtbar), 48 kHz mono i16, Pausen werden ausgelassen, Lücken mit Stille gefüllt.
- Zeiger-Sidecar: ~30 Hz (Intervall 33 ms), max 108 000 Samples, normierte Koordinaten `cx,cy ∈ [0,1]` relativ zum aufgenommenen Inhalt, `visible=false` nach 3 aufeinanderfolgenden Samples außerhalb, 16 Zeigertypen (Liste aus OpenScreen `NativeCursorType`), Klick-Events aus Linksdruck/-loslassen.
- Countdown 3‑2‑1, je 1 s, abschaltbar.
- Projektordner Standard `~/Movies/Inspector Rust/`, Setting `record.project_dir`; Exporte (spätere Etappe) nach Downloads.
- Hotkey bleibt `ActionId::Record`, Settings-Key `hotkey.record`, Default `Ctrl+Shift+Alt+KeyS` — zeigt jetzt auf `record` (Badge-Zahl der Hotkeys bleibt 13).
- Alle UI-Texte in `en.json` **und** `de.json`; CommandDoc mit `tagline_de`; `features.txt`-Zeile; Animationsregeln aus `.claude/rules/animation.md` (Tokens, kein `transition-all`).
- Eigene Fenster (HUD, Countdown, Leiste) dürfen nie in der Aufnahme erscheinen: `setSharingType: 0` + Ausschluss im `SCContentFilter`.
- Kein neues npm-Paket.

## Review Focus

1. **Pause/Fortsetzen erzeugt nicht-steigende Zeitstempel** → AVAssetWriter scheitert still ab dem nächsten Frame. Erwartung: ein Frame wird verworfen, die Aufnahme läuft weiter. (Task 2 `gate_*`-Tests, Task 6 Live-Test mit 5 Pausen.)
2. **Retina-Display / ungerade Fenstergrößen / Ultrawide über 4K** → schwarze Ränder oder Encoder-Fehler. Erwartung: Puffer hat exakt das Seitenverhältnis der Quelle, gerade Maße. (Task 2 `output_size_*`.)
3. **Abbruch mitten in der Aufnahme** (App beendet, Writer-Fehler, Platte voll) → Erwartung: der Projektordner bleibt erhalten, `project.json` trägt `status: "failed"`/`"incomplete"` + Grund, das Panel zeigt ihn als beschädigt statt ihn zu verschweigen. (Task 3 `incomplete_project_is_listed_with_reason`, Task 7.)
4. **Mikrofon wird abgezogen oder wechselt mitten in der Aufnahme** → Erwartung: `mic.wav` bleibt zeitlich synchron zum Video (Stille statt Lücke). (Task 4 `gap_is_filled_with_silence`.)
5. **Zeiger auf anderem Bildschirm / außerhalb des Fensters** → Erwartung: `visible=false` nach 3 Samples, kein Sprung an den Rand; bei verweigerter Bedienungshilfen-Freigabe läuft die Aufnahme weiter mit `provider: "position-only"`. (Task 2 `visibility_*`, Task 5.)

---

### Task 1: Gemeinsame Helfer aus `screen_record.rs` herauslösen

Ohne Verhaltensänderung, damit Task 10 den alten Rekorder löschen kann.

**Files:**
- Create: `core/rust-lib/src/ffmpeg_util.rs`, `core/rust-lib/src/display_rects.rs`
- Modify: `core/rust-lib/src/screen_record.rs` (L541 `ffmpeg_path`, L1137-1195 `cg_displays`), `lib.rs` (`mod`-Liste), Aufrufer `social_dl.rs:496`, `media_trim.rs:74,91`, `audio_swap.rs:218,230,252,283`, `transcribe.rs:577`, `commands.rs:4972`

**Interfaces:**
- Produces: `crate::ffmpeg_util::ffmpeg_path() -> Option<PathBuf>`, `crate::ffmpeg_util::ffprobe_path() -> Option<PathBuf>`; `crate::display_rects::physical_rects() -> Vec<(i32, i32, u32, u32)>` (macOS).

- [ ] **Step 1:** Funktionen samt ihren bestehenden Tests verschieben; `screen_record.rs` ruft die neuen Pfade auf.
- [ ] **Step 2:** `cargo test -p inspector-rust-core --lib` → alle grün, gleiche Anzahl Tests.
- [ ] **Step 3:** `bash scripts/check.sh` → grün.
- [ ] **Step 4:** Commit `refactor: move ffmpeg_path and display rects out of screen_record`.

---

### Task 2: Reiner Kern `record::core`

**Files:**
- Create: `core/rust-lib/src/record/mod.rs` (Re-Exports, `#[cfg(target_os="macos")]`-Untermodule), `core/rust-lib/src/record/core.rs` (inkl. Tests am Dateiende)
- Modify: `core/rust-lib/src/lib.rs` (`mod record;`)

**Interfaces (Produces):**
- `pub fn capture_output_size(content_w_pt: f64, content_h_pt: f64, point_pixel_scale: f64, max_w: u32, max_h: u32) -> (u32, u32)` — Port von OpenScreens `captureOutputSize` (ein Faktor für beide Achsen, `even_floor` min 2; unbrauchbare Eingabe → Deckel).
- `pub fn default_video_bitrate(w: u32, h: u32, fps: u32) -> u32` und `pub fn key_frame_interval(fps: u32) -> u32` (= `max(1,fps)`).
- `pub struct VideoTimestampGate { last: Option<i64>, rejected: u32 }` mit `check(pts_ticks: Option<i64>) -> GateVerdict` (`Admit | Invalid | NotAfterPrevious`) und `record(pts_ticks: i64)`; Ticks = Zeit in der Zeitbasis der Samples (Port von `VideoTimestampGate.swift`).
- `pub struct PauseClock` mit `pause(now_ns: i64)`, `resume(now_ns: i64)`, `is_paused() -> bool`, `offset_ns() -> i64`, `timeline_ns(now_ns: i64) -> i64` (eingefroren während Pause).
- `pub struct PauseShared(Mutex<PauseClock>)` mit `pause/resume/is_paused/offset_ns/timeline_ns` (dünne Hülle, von Capture, Mikro und Zeiger gemeinsam benutzt) und `pub fn host_now_ns() -> i64` in `mod.rs` (macOS: `mach_absolute_time` × Timebase — dieselbe Uhr wie die CMSampleBuffer-Zeitstempel des Host-Clocks; sonst `Instant`-basiert).
- `pub struct CursorVisibility { outside_run: u8 }` mit `observe(inside: bool) -> bool` (sichtbar bis zum 3. Draußen-Sample in Folge).
- `pub fn normalize_point(px: f64, py: f64, bounds: (f64, f64, f64, f64)) -> (f64, f64, bool)` — `(cx, cy, inside)`, Werte NICHT geklemmt (außerhalb darf >1 sein).
- `pub fn cursor_type_for_roles(roles: &[(Option<String>, Option<String>, Option<String>)]) -> Option<CursorType>` — Liste von (role, subrole, roleDescription lowercased) vom Element aufwärts, max 5 Ebenen; Regeln wörtlich aus OpenScreen `isTextInputRole`/`isPointerRole`.
- `pub enum CursorType` mit den 16 Werten, serde `kebab-case` (`arrow`, `text`, `pointer`, `crosshair`, `open-hand`, `closed-hand`, `resize-ew`, `resize-ns`, `resize-nesw`, `resize-nwse`, `move`, `not-allowed`, `wait`, `app-starting`, `help`, `up-arrow`).
- `pub fn project_dir_name(now: chrono::DateTime<chrono::Local>, title: &str, taken: &dyn Fn(&str) -> bool) -> String` — `YYYY-MM-DD HH.MM.SS Titel` (Titel sanitisiert, leer = „Aufnahme“/„Recording“ nach Sprache), Kollision → ` 2`, ` 3` …
- `pub fn disk_space_verdict(free_bytes: u64, est_bytes_per_min: u64) -> DiskVerdict` (`Ok | Low { minutes: u64 } | Refuse`) — Refuse unter 1 GiB frei, Low unter 15 Minuten Reserve.

- [ ] **Step 1: Failing tests** (in `core.rs`):
  - `output_size_retina_display`: `(1512.0, 982.0, 2.0, 3840, 2160)` → `(3024, 1964)`.
  - `output_size_caps_proportionally`: `(5120.0,1440.0,1.0,3840,2160)` → `(3840,1080)`.
  - `output_size_is_even_and_positive`: `(1001.0, 3.0, 1.0, 3840, 2160)` → beide gerade, ≥ 2.
  - `output_size_garbage_falls_back_to_ceiling`: `(0.0, f64::NAN, 2.0, 3840, 2160)` → `(3840, 2160)`.
  - `bitrate_clamps`: `(1920,1080,60)` → `12_441_600`; `(320,240,10)` → `2_000_000`; `(7680,4320,60)` → `60_000_000`.
  - `gate_rejects_equal_and_earlier_but_not_after`: admit 100 → record 100 → check 100 = NotAfterPrevious, check 99 = NotAfterPrevious, check 101 = Admit; `rejected == 2`.
  - `gate_check_does_not_move_last`: check 200 (admit) ohne record → check 150 = Admit.
  - `gate_invalid_none`: `check(None)` = Invalid.
  - `pause_clock_freezes_and_shifts`: resume nach 2 s Pause → `offset_ns == 2e9`; `timeline_ns` während Pause konstant.
  - `visibility_hides_after_third_outside_sample`: `[false,false]` → sichtbar, 3. false → unsichtbar, ein `true` → sofort wieder sichtbar.
  - `cursor_type_text_and_pointer`: `AXTextField` → Text; Parent `AXButton` an Ebene 2 → Pointer; subrole mit „link“ → Pointer; 6 Ebenen ohne Treffer → None (nur 5 werden geprüft).
  - `project_dir_name_collides`: `taken` für Name ohne Suffix true → Ergebnis endet auf ` 2`; Titel mit `/` und `:` wird bereinigt.
  - `disk_verdict_thresholds`: 500 MiB → Refuse; 3 GiB bei 300 MiB/min → Low{10}; 50 GiB → Ok.
- [ ] **Step 2:** `cargo test -p inspector-rust-core --lib record::` → FAIL (nicht definiert).
- [ ] **Step 3:** Implementieren.
- [ ] **Step 4:** Tests grün; Mutationsprobe: `even_floor` entfernen und Gate-Vergleich `<=` → `<` je einmal → jeweils rot sehen, zurücksetzen (Datei vorher committen).
- [ ] **Step 5:** Commit `feat(record): pure capture core (sizing, bitrate, timestamp gate, pause clock)`.

---

### Task 3: Projekt- und Sidecar-Format v1

**Files:**
- Create: `core/rust-lib/src/record/project.rs`

**Interfaces (Produces):**
- `pub const PROJECT_FILE: &str = "project.json"`, `CURSOR_FILE = "cursor.json"`, `SCREEN_FILE = "screen.mp4"`, `MIC_FILE = "mic.wav"`, `PROJECT_SCHEMA: u32 = 1`.
- `pub struct Project { schema: u32, id: String, title: String, created_at: i64, updated_at: i64, status: ProjectStatus, failure: Option<String>, recording: RecordingMeta, editor: serde_json::Value }` — `editor` ist ein für spätere Etappen reservierter Block (`{}`), wird beim Lesen/Schreiben unverändert durchgereicht.
- `pub enum ProjectStatus { Recording, Ready, Incomplete, Failed }` (serde lowercase).
- `pub struct RecordingMeta { source: SourceKind /* display|window */, source_label: String, width: u32, height: u32, fps: u32, duration_ms: u64, system_audio: bool, mic: Option<MicMeta { offset_ms: i64, sample_rate: u32 }>, cursor_mode: CursorMode /* editable-overlay|system */, pauses: Vec<(u64,u64)> }`.
- `pub struct CursorRecording { version: u32 /*=1*/, provider: CursorProvider /* native|position-only|none */, samples: Vec<CursorSample>, assets: Vec<CursorAsset> }`; `CursorSample { time_ms, cx, cy, visible: Option<bool>, asset_id: Option<String>, cursor_type: Option<CursorType>, interaction_type: Option<Interaction /* move|click|mouseup */> }` (serde camelCase, Feldnamen wie OpenScreen `contracts.ts`); `CursorAsset { id, image_png_b64, width, height, hotspot_x, hotspot_y, scale_factor }`.
- `pub fn write_atomic(dir: &Path, name: &str, value: &impl Serialize) -> io::Result<()>` (tmp + fsync + rename).
- `pub fn read_project(dir: &Path) -> Result<Project, String>` — `schema > PROJECT_SCHEMA` → Fehler „neuere Version“; fehlende Felder → serde-Defaults.
- `pub fn list_projects(root: &Path) -> Vec<ProjectSummary { dir, title, created_at, duration_ms, status, failure }>` — neueste zuerst; Ordner ohne lesbare `project.json` erscheinen als `Failed` mit Grund, nie übersprungen.

- [ ] **Step 1: Failing tests:** `project_roundtrip_keeps_editor_block` (beliebiges `editor`-JSON kommt bitgleich zurück) · `newer_schema_is_refused` · `incomplete_project_is_listed_with_reason` (Ordner mit `status: recording` + ohne `screen.mp4` → `Incomplete`; Ordner mit kaputter JSON → `Failed`) · `list_is_newest_first` · `cursor_json_uses_openscreen_field_names` (serialisiertes Sample enthält `"timeMs"`, `"cursorType"`, `"interactionType"`, `"assetId"`) · `write_atomic_leaves_no_tmp` (Tempdir).
- [ ] **Step 2:** FAIL. **Step 3:** implementieren. **Step 4:** grün.
- [ ] **Step 5:** Commit `feat(record): versioned project and cursor sidecar format`.

---

### Task 4: Mikrofon → WAV mit zeitlicher Ausrichtung

**Files:**
- Create: `core/rust-lib/src/record/wav.rs` (rein), `core/rust-lib/src/record/mic.rs` (cpal-Rand)

**Interfaces (Produces):**
- `pub fn wav_header(sample_rate: u32, channels: u16, data_bytes: u32) -> [u8; 44]` (PCM 16 bit).
- `pub struct WavSink` mit `create(path, sample_rate) -> io::Result<Self>`, `push(&mut self, at_ns: i64, mono_f32: &[f32])` (resampelt auf 48 kHz über `shazam::resample`-artigen Linearresampler — bestehenden `resample_to_16k_i16` NICHT nehmen, eigener `resample_linear(input, from, to)` mit Test), füllt Lücken > 50 ms zwischen erwarteter und tatsächlicher Position mit Stille, `finish(self) -> io::Result<u64 /*ms*/>` schreibt Header-Größen nach.
- `pub fn start_mic(app: AppHandle, sink: Arc<Mutex<WavSink>>, clock: Arc<PauseShared>) -> MicStream` — nutzt `mic_capture::start_chunks`; während Pause verworfen; `at_ns` = Host-Uhr minus Pausenversatz.

- [ ] **Step 1: Failing tests:** `header_matches_canonical_layout` (RIFF/WAVE/fmt /data, Byte-Raten) · `gap_is_filled_with_silence` (zwei Pushes 200 ms auseinander mit je 10 ms Audio → Datei-Länge ≈ 210 ms ± 1 Frame) · `finish_patches_sizes` (Tempdir, Header-Größen = Dateigröße − 8 / − 44) · `resample_linear_preserves_duration` (44,1 k → 48 k: 441 Samples → 480).
- [ ] **Step 2–4:** FAIL → implementieren → grün.
- [ ] **Step 5:** Commit `feat(record): microphone to time-aligned WAV`.

---

### Task 5: Zeiger-Sampler (macOS)

**Files:**
- Create: `core/rust-lib/src/record/cursor_macos.rs`

**Interfaces:**
- Consumes: `PauseShared`, `host_now_ns`, `CursorVisibility`, `normalize_point`, `cursor_type_for_roles`, `CursorRecording`/`CursorSample`/`CursorAsset` (Tasks 2–3).
- Produces: `pub struct CursorRecorder` mit `start(bounds_pt: (f64,f64,f64,f64), clock: Arc<PauseShared>) -> Self`, `set_bounds(..)` (Fenster bewegt sich), `stop(self) -> CursorRecording`.

Umsetzung: Thread `ir-record-cursor`, alle 33 ms: Position aus `CGEventGetLocation(CGEventCreate(null))` (globale Top-Left-Punkte, wie `cursor_wrap`), Zeitstempel = Timeline-ms aus `PauseShared` (während Pause kein Sample). Klicks: listen-only `CGEventTap` auf LeftMouseDown/Up (Muster `window_snap/macos.rs`, eigener CFRunLoop im selben Thread per `CFRunLoopRunInMode(…, 0.001)`), Zähler je Tick abholen → `interaction_type` `click` bei Down, `mouseup` bei Up, sonst `move`. Zeigertyp: nur wenn `expander::accessibility_granted()`, AX-Hit-Test wie `window_palette/macos.rs` (Messaging-Timeout 0,05 s), Rollen-Strings lesen (neuer kleiner Helfer `ax_string(el, attr) -> Option<String>`), ≤ 5 Eltern. Zeigerbild: `NSCursor currentSystemCursor` → PNG via `NSBitmapImageRep`, SHA-256 (sha2) als `assetId`, nur beim ersten Auftreten ins `assets`-Array. Ohne Accessibility: `provider = "position-only"`, keine Klicks/Typen. Samples gedeckelt bei 108 000.

- [ ] **Step 1:** Ignorierter Live-Test `cursor_recorder_live` (`cargo test … cursor_recorder_live -- --ignored --nocapture`): 2 s aufnehmen, prüfen ≥ 50 Samples, Zeitstempel streng steigend, mindestens ein Asset; gibt Provider + Anzahl aus.
- [ ] **Step 2:** Implementieren; Live-Test lokal ausführen, Ausgabe in den Commit-Text.
- [ ] **Step 3:** Commit `feat(record): cursor telemetry sampler with click and cursor-type capture`.

---

### Task 6: Video + Systemton aufnehmen (SCStream → AVAssetWriter)

**Files:**
- Create: `core/rust-lib/src/record/capture_macos.rs`
- Modify: `core/rust-lib/Cargo.toml` (macOS-Target: `objc2-av-foundation = { version = "0.3.2", default-features = false, features = ["std","AVAssetWriter","AVAssetWriterInput","AVMediaFormat","AVVideoSettings","AVAudioSettings", …nur benötigte] }`; bei `objc2-screen-capture-kit` Features `"SCContentSharingPicker"`, `"SCStreamConfiguration"` ergänzen falls nötig)

**Interfaces:**
- Consumes: `capture_output_size`, `default_video_bitrate`, `key_frame_interval`, `VideoTimestampGate`, `PauseClock` (Task 2).
- Produces: `pub struct Capture` mit `start(filter: Retained<SCContentFilter>, opts: CaptureOptions { system_audio: bool, show_system_cursor: bool, fps: u32 }, out: &Path, clock: Arc<PauseShared>) -> Result<Capture, String>`, `stop(self) -> Result<CaptureResult { width, height, duration_ms, rejected_frames }, String>`, `content_bounds_pt(&self) -> (f64,f64,f64,f64)`.

Umsetzung strikt nach `ScreenCaptureRecorder.swift` `setupWriter`/`makeStreamConfiguration`/`retimedSampleBuffer`: Größe aus `filter.contentRect × pointPixelScale` über `capture_output_size(…, 3840, 2160)`; `minimumFrameInterval = 1/fps`, `queueDepth = 6`, Pixelformat 420v, `showsCursor = show_system_cursor`; Audio 48 kHz/2 ch/`excludesCurrentProcessAudio`. Writer: `.mp4`, H.264-Settings exakt wie Global Constraints, `expectsMediaDataInRealTime = true`, Session-Start beim ersten zugelassenen Frame. Während Pause Samples verwerfen; danach PTS um `offset` zurückversetzen (`CMSampleBufferCreateCopyWithNewTiming`), Video durch das Gate. Nur vollständige Frames (`SCStreamFrameInfo.status == complete`). Writer-Fehler → `CaptureResult`-Fehler mit `writer.error`-Text, nie still. Eigene Fenster: `excludingWindows` mit den Fensternummern von HUD/Leiste/Countdown/Popup.

- [ ] **Step 1:** Ignorierter Live-Test `capture_live_with_pauses`: Hauptbildschirm 4 s mit 5× Pause/Resume à 100 ms, dann `ffprobe` (über `ffmpeg_util::ffprobe_path`) → Videospur H.264, Audiospur AAC 48 kHz, Dauer ≈ 3,5 s ± 0,3, `rejected_frames` ausgegeben.
- [ ] **Step 2:** Implementieren; Live-Test ausführen, `ffprobe`-Kurzausgabe in den Commit-Text.
- [ ] **Step 3:** Commit `feat(record): ScreenCaptureKit capture to H.264/AAC via AVAssetWriter`.

---

### Task 7: Quellwahl + Sitzung + IPC

**Files:**
- Create: `core/rust-lib/src/record/picker_macos.rs`, `core/rust-lib/src/record/session.rs`
- Modify: `core/rust-lib/src/commands.rs` (neue Befehle), `core/rust-lib/src/lib.rs` (`invoke_handler`, `app.manage(record::RecordState::default())`, Start-Aufräumen: Projekte mit `status: recording` → `incomplete`)

**Interfaces:**
- Consumes: Tasks 2–6.
- Produces (IPC, alle `async` + `spawn_blocking` wo blockierend):
  - `record_status() -> RecordStatus { supported: bool, reason: Option<String>, phase: "idle"|"picking"|"countdown"|"recording"|"paused"|"finishing", elapsed_ms: u64, project_dir: Option<String> }`
  - `record_open_hud()` — baut/zeigt Fenster `record-hud` (Worker-Thread → Hauptthread, Muster `screenshot_editor::open_editor`), Popup wird vorher versteckt.
  - `record_pick_source(kind: "display"|"window")` — zeigt `SCContentSharingPicker` (Hauptthread, Observer-Klasse per `define_class!`), Ergebnis als Event `record-source-picked { label, kind }` bzw. `record-pick-cancelled`.
  - `record_start(opts: StartOptions { system_audio, mic, cursor_mode: "editable-overlay"|"system", countdown: bool, title: String })` — Platzprüfung (`disk_space_verdict`, Refuse → Fehler `record.disk_full`), Projektordner anlegen + `project.json` (`status: recording`), optional Countdown-Fenster `record-countdown` (3 s), dann Capture + Mic + Cursor starten, Displayschlaf per `IOPMAssertionCreateWithName("PreventUserIdleDisplaySleep")` verhindern, Leiste `record-bar` öffnen, `Sound::RecordStart`.
  - `record_pause()`, `record_resume()`, `record_stop() -> String /*project dir*/`, `record_cancel()` (löscht den Ordner).
  - `record_list_projects() -> Vec<ProjectSummary>`, `record_reveal_project(dir)` (über `reveal_in_file_manager`), `record_delete_project(dir)` (Papierkorb via `trash`, nur Ordner unterhalb des Projekt-Wurzelordners — Pfadprüfung mit Test).
  - `get_record_config() / set_record_config(cfg: RecordConfig { project_dir, countdown, cursor_mode, system_audio, mic })` — Settings-Keys `record.project_dir|countdown|cursor_mode|system_audio|mic`.
  - Events: `record-status` (bei jedem Phasenwechsel + 1×/s Elapsed), `record-finished { dir, status, failure }`.
- Stop schreibt `project.json` endgültig (`ready` oder `failed` + Grund), `cursor.json`, Pausenliste; danach `Sound::RecordStop`, Leiste schließen, Projektordner im Finder zeigen.

- [ ] **Step 1: Failing tests (rein, in `session.rs`):** `phase_transitions_are_guarded` (pure `next_phase(phase, action) -> Result<Phase>`: Start aus `recording` → Fehler „läuft bereits“; Pause aus `idle` → Fehler; Stop aus `paused` erlaubt) · `delete_refuses_paths_outside_root` (`../`, Symlink-freier absoluter Fremdpfad, Root selbst → abgelehnt).
- [ ] **Step 2–4:** FAIL → implementieren → grün.
- [ ] **Step 5:** Manuell: `pnpm dev:macos`, `record_open_hud` aus der Konsole aufrufen, Picker erscheint. Commit `feat(record): source picker, session lifecycle and IPC`.

---

### Task 8: Fenster HUD, Countdown, Steuerleiste

**Files:**
- Create: `core/frontend/src/components/RecordHud.tsx`, `RecordCountdown.tsx`, `RecordBar.tsx`, `RecordNotes.tsx`, `core/frontend/src/lib/record.ts` (+ `record.test.ts`), `RecordBar.test.tsx`
- Modify: `core/frontend/src/main.tsx` (Routen `record-hud`, `record-countdown`, `record-bar`, `record-notes`), `core/frontend/src/lib/ipc.ts` (typisierte Wrapper zu Task 7), alle drei `*/src-tauri/capabilities/default.json` (Labels ergänzen; `core:window:allow-set-size` ist bereits explizit da), `locales/en.json` + `de.json` (`rec2.*`)

**Interfaces:**
- Consumes: IPC + Events aus Task 7.
- Produces: `lib/record.ts`: `formatElapsed(ms: number): string` (`m:ss`, ab 1 h `h:mm:ss`), `phaseLabelKey(phase): MessageKey`, `canStart(state): boolean` (Quelle gewählt und Phase idle).

UI: **HUD** (kleines, verschiebbares Fenster mittig unten am Cursor-Monitor): zwei große Knöpfe „Bildschirm“/„Fenster“ (öffnen den System-Picker), danach gewählte Quelle als Chip, Schalter Systemton · Mikrofon · Zeiger nachträglich bearbeitbar (= `editable-overlay`) · Countdown, Titelfeld, „Aufnehmen“ (Enter), Esc = abbrechen. **Countdown**: Vollflächig transparent, große Ziffer 3‑2‑1 mit Skalen-/Opacity-Animation (`--duration-base`, reduced-motion → statische Ziffer), click-through. **Notizen** (Teleprompter, wie OpenScreens Notes-Fenster): Knopf im HUD öffnet Fenster `record-notes` (schwebend, verschiebbar, Textfeld + Schriftgröße + optionales Auto-Scrollen mit Tempo-Regler), Text bleibt in Setting `record.notes`; nie in der Aufnahme (Ausschlussliste aus Task 7 um dieses Label erweitern). **Leiste**: wie die alte Stop-Leiste (Pulspunkt, Zeit, Pause/Fortsetzen, Stopp, einklappbar 36×30, Ziehgriff) — Muster von `RecordStopBar.tsx` übernehmen (inkl. `setSize`-Einklappen); hört auf `record-status`.

- [ ] **Step 1: Failing tests:** `formatElapsed` (0 → `0:00`, 61 000 → `1:01`, 3 661 000 → `1:01:01`) · `canStart` · `RecordBar.test.tsx`: Klick „Pause“ ruft `recordPause`, Status `paused` zeigt „Fortsetzen“ und stoppt den Zeittakt (fake timers).
- [ ] **Step 2–4:** FAIL → implementieren → grün (`pnpm --filter inspector-rust-frontend exec vitest run src/lib/record.test.ts src/components/RecordBar.test.tsx`).
- [ ] **Step 5:** `pnpm typecheck && pnpm lint` grün. Commit `feat(record): HUD, countdown and control bar windows`.

---

### Task 9: Der `record`-Befehl, Hotkey, Tray, Settings, Doku

**Files:**
- Create: `core/frontend/src/components/RecordPanel.tsx` (+ `RecordPanel.test.tsx`), `core/frontend/src/components/RecordSettingsSection.tsx`
- Modify: `lib/commands.ts` (Kind `record`, Keyword `record`, `requiresArg: false`, `platform: ["mac"]`, `description(_de)`; `commands.test.ts` Zähler 131 → 132), `App.tsx` (`recordMode/recordFocus`, `PANEL_KINDS` + Auto-Exit, `commandEntry`-Case `cmd.record.label/hint`, Dispatch: Enter → `recordOpenHud()`), `lib/commandDocs.ts` (CommandDoc `record`, Kategorie `CAT_MEDIA`, ≥ 3 Beispiele, ≥ 1 Tipp, Caveat „nur Apple Silicon, macOS 14+“, related `["shot","trim"]`), `lib/settings-sections.ts` (`sec("record", ["record","aufnahme","recording","video"])` HINTEN anhängen, in Kategorie `commands`), `SettingsPanel.tsx` (`blocks.record`), `core/rust-lib/src/hotkey.rs` (`ActionId::Record` → `record_open_hud`), `lib.rs` Tray-Eintrag `"record"` unverändert verdrahtet über `dispatch_action`, `features.txt` (Zeile unter „Suchleisten-Commands“: ``Aufnahme — `record` / ⌃⇧⌥S: Bildschirm oder Fenster mit Systemton, Mikro und Zeigerdaten als nachbearbeitbares Projekt``), Locales.

**Interfaces:**
- Consumes: `recordListProjects`, `recordRevealProject`, `recordDeleteProject`, `recordStatus`, `recordOpenHud`, `get/setRecordConfig`.
- Produces: Panel zeigt beim Tippen (show-while-typed, ruft nur `record_list_projects`/`record_status`, öffnet kein Mikro) die letzten Projekte (Titel, Datum, Dauer, Status-Chip „unvollständig/fehlgeschlagen“ mit Grund); läuft eine Aufnahme, zeigt es Phase + Zeit + Stopp. Enter = neue Aufnahme (HUD), ↑/↓ wählt Projekt, ⌘⏎ zeigt im Finder, Entf = zweistufig löschen (inline, nie `window.confirm`).

- [ ] **Step 1: Failing tests:** `RecordPanel.test.tsx`: fehlerhaftes Projekt zeigt seinen Grund · Entf beim ersten Druck armiert nur, zweiter Druck ruft `recordDeleteProject` · bei `supported:false` steht der Grund statt der Liste. Plus die bestehenden Doku-Gates (`commandDocs.test.ts`, `i18n.test.ts`, `settings-sections.test.ts`, `website-features.test.ts`).
- [ ] **Step 2–4:** FAIL → implementieren → grün; `node scripts/gen-docs.mjs` ausführen.
- [ ] **Step 5:** `bash scripts/check.sh` + `pnpm test` grün. Commit `feat(record): record command, panel, settings and hotkey`.

---

### Task 10: Alten Rekorder entfernen, Doku nachziehen, installieren

**Files:**
- Delete: `core/rust-lib/src/screen_record.rs`, `core/frontend/src/components/RecordOverlay.tsx`, `RecordStopBar.tsx`
- Modify: `commands.rs` (L7042-7329 bis auf `reveal_in_file_manager`), `lib.rs` (`mod screen_record`, `RecordState`-manage, `cleanup_orphans`, 7 invoke-Einträge), `main.tsx` (Routen/Imports), `ipc.ts` (L3825-3871), Capabilities (Labels `record-overlay`, `record-stop` raus), `styles.css` (`.recordPulse` nur entfernen, falls `RecordBar` sie nicht übernommen hat), `FeaturesPanel.tsx:140-144` (Zeile zeigt jetzt auf `record`), Locales (`rec.*`, `feat.drag_a_region_pick_audio` raus), `audio.rs` (`loopback_capture_target`, `current_default_output_id` raus, falls ohne weitere Aufrufer), `Cargo.toml` Windows-Feature `Win32_System_Console` nur entfernen, wenn `cargo check --target x86_64-pc-windows-gnu` (mit `ORT_SKIP_DOWNLOAD=1`) ohne es keine neuen Fehler zeigt, `features.txt:52`, `README.md`/`README.de.md` (Hotkey-Tabelle, Feature-Tabelle, Modulbaum), `CLAUDE.md` (Abschnitt „Screen recording“ ersetzen durch „record — Aufnahme (Etappe 1)“ inkl. Fallstricke aus Tasks 5–7), `website/index.md:24` + `website/llms.txt:41`, `scripts/install-macos.sh` + `scripts/pack-macos-release-dmg.sh` (`NSMicrophoneUsageDescription`-Text nennt auch Aufnahmen), `CHANGELOG.md` (neue Version, Englisch).

- [ ] **Step 1:** Löschen + Verweise entfernen; `grep -rn "screen_record\|RecordOverlay\|RecordStopBar\|startScreenRecord\|record-overlay\|record-stop" core macos win linux scripts` → nur noch CHANGELOG-Historie.
- [ ] **Step 2:** `cargo test --workspace`, `pnpm test` (Badges aktualisieren sich über den posttest-Hook), `bash scripts/check.sh` → grün.
- [ ] **Step 3:** Ende-zu-Ende auf dem Mac: `scripts/install-macos.sh`; `record` tippen → Panel; Enter → HUD; Fenster wählen; 20 s mit Systemton + Mikro + 3 Pausen aufnehmen, dabei klicken und über ein Textfeld fahren; Stopp → Finder zeigt Ordner; prüfen: `ffprobe screen.mp4` (H.264 + AAC), `mic.wav` Dauer ≈ Videodauer (± 0,2 s), `cursor.json` hat `click`-Samples und `cursorType: "text"`; HUD/Leiste sind im Video nicht zu sehen; ⌃⇧⌥S öffnet das HUD.
- [ ] **Step 4:** Commit `feat(record): replace the ffmpeg region recorder` (Version bump nach Hausregel), push.

---

## Nach Etappe 1

Etappe 2 (Compositor-Helfer `ir-record-render`, Editor-Fenster, Trim/Tempo/Crop, MP4/GIF-Export) bekommt einen eigenen Plan, sobald diese Etappe installiert und von dir ausprobiert ist.
