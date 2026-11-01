# `record` Etappe 2 — Compositor, Editor, Export — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ein aufgenommenes `record`-Projekt öffnet sich in einem Editor-Fenster mit Live-Vorschau und Zeitleiste (Schneiden, Tempo 0,25–16×, Zuschneiden mit Presets), wird im Projekt gespeichert und als MP4 oder GIF nach Downloads exportiert — gerendert vom vendorten OpenScreen-Compositor in einem eigenen Hilfsprozess.

**Architecture:** Der OpenScreen-Compositor (MIT, v2.1.2 `bfd5bc32`, Metal + VideoToolbox über ffmpeg 8.1.2 LGPL-shared) wird als Workspace-Crate `vendor/openscreen-compositor` übernommen und nur vom neuen Hilfsprogramm `ir-record-render` gelinkt (eigener Prozess, Muster `ir-ocr`; die Tray-App linkt weder ffmpeg noch Metal-Compositor). App ↔ Helfer sprechen ein gerahmtes Protokoll über stdin/stdout (JSON-Befehle, Binärrahmen für Vorschaubilder). Das Editor-Dokument (Schnitte, Tempo, Crop, Ausgabe) ist reines Rust in `record/editor.rs` und wird dort in das Compositor-`Scene`-JSON übersetzt; die Oberfläche ist ein Tauri-Fenster `record-editor`.

**Tech Stack:** Rust (vendorter Compositor: metal 0.29, objc 0.2, bindgen 0.70, naga 24; Helfer-Binary), ffmpeg 8.1.2 aus Quellcode (LGPL, shared), Tauri 2 (`ipc::Response` für Rohbilder), React 19 + TS, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (Etappe 2; Entscheidungen 0, 1, 4). Recon des Compositors: in Task 1 nach `vendor/openscreen-compositor/RECON.md` kopieren (Quelle: Scratchpad-Datei `compositor-recon.md` dieser Sitzung). Referenzquelle (nur lesen, nie ausführen): Klon `openscreen-src/new` (Pfad in RECON.md).

## Global Constraints

- Nur macOS Apple Silicon: Compositor-Crate und Helfer nur für `cfg(all(target_os = "macos", target_arch = "aarch64"))`; Linux-/Windows-/Intel-Builds bleiben unverändert grün (Crate dort nicht im Abhängigkeitsgraphen).
- ffmpeg **8.1.2**, aus Quellcode, `--enable-shared --disable-static --disable-doc --disable-debug --disable-xlib --disable-libxcb --disable-sdl2 --disable-lzma --enable-videotoolbox --enable-audiotoolbox --disable-x86asm --arch=arm64 --cc=clang -mmacosx-version-min=13.0`, **nie** `--enable-gpl`/`--enable-nonfree`; Tarball-SHA256 fest im Skript. Dylibs ins Bundle `Contents/Frameworks/`, `install_name` auf `@rpath`, keine absoluten Build-Pfade, jede Dylib signiert. LGPL-Hinweis + Quellverweis im Bundle.
- Compositor ohne `segmentation`-Feature (`default-features = false`); wgpu/pollster/cosmic-text nur für Linux; bindgen-Ziel aus `CARGO_CFG_TARGET_ARCH`, nicht hart `aarch64`.
- Lizenz: OpenScreen `LICENSE` (MIT) + Recordly-Hinweis (MIT) wörtlich im Vendor-Ordner; Herkunft (Repo, Commit `bfd5bc32`) + jede eigene Änderung in `vendor/openscreen-compositor/VENDORED.md`.
- Vorschau und Export laufen durch denselben Compositor-Code (Anti-Ziel „Vorschau ≠ Export“).
- Tempo 0,25–16×; Crop-Presets frei, 16:9, 9:16, 1:1, 4:3, 3:4, 21:9; MP4 720p/1080p/Quelle, 24/30/60 fps, H.264 oder HEVC (VideoToolbox); GIF 15–30 fps, Breiten 480/640/854/1080, Loop an/aus. Exporte nach `~/Downloads`, eindeutiger Name, im Finder zeigen.
- Projektformat bleibt `PROJECT_SCHEMA = 1`; der Editor füllt den reservierten Block `project.json → editor` (eigene `version: 1` darin). Ältere Projekte ohne `editor`-Inhalt öffnen mit Standard (ein Clip über die ganze Länge, Tempo 1, kein Crop).
- Mikrofon: `mic.wav` bleibt eigene Datei; im Export mit eigenem Pegel (dB, −60…+12, Standard 0) gemischt und mit dem Tempo gestreckt — keine Drift.
- Helfer startet erst beim ersten Öffnen des Editors (kein Einfluss auf App-Start/Popup-Latenz); ein Absturz des Helfers beendet nie die App, der Editor zeigt einen Fehler und kann neu starten.
- Alle UI-Texte en+de; Animationsregeln `.claude/rules/animation.md`; kein neues npm-Paket.
- Bewusste Abweichungen vom Spec-Inventar (Entscheidung): VP9-Export entfällt (VideoToolbox kann es nicht, OpenScreen lehnt es auf macOS ebenfalls ab); der Limiter folgt dem Compositor (−2 dBTP statt der im Spec notierten −1,5 dBFS); mehrere Aufnahmen in einem Projekt (einfügen/umordnen/duplizieren) folgen in Etappe 4.
- Laufwerk ist knapp: jeder Implementierer nutzt `CARGO_INCREMENTAL=0` und prüft `df -h /System/Volumes/Data` vor schweren Builds (Abbruch unter 1,5 GB).

## Review Focus

1. **Helfer stirbt mitten in Vorschau oder Export** (Panic, Metal-Fehler, kaputte Datei) → Erwartung: Editor bleibt bedienbar, Meldung mit Grund, „Neu starten“; ein halber Export bleibt nicht als Datei liegen. (Task 4 `panic_is_reported_not_fatal`, Task 5 `client_survives_helper_crash`.)
2. **Tempo-Abschnitt über Mikrofon-Sprache** → Erwartung: Mikro bleibt lippensynchron zum Bild (gestreckt wie der Systemton). (Task 2 `mic_follows_speed_region`.)
3. **Schnitte bis an die Ränder / winzige Clips / überlappende Tempo-Bereiche** → Erwartung: Dokument wird normalisiert (Clips ≥ 0,1 s, Bereiche innerhalb ihres Clips, Überlappung → spätere gewinnt), nie ein Compositor-Fehler. (Task 3 `normalize_*`.)
4. **Projekt aus Etappe 1 ohne Editor-Block, oder mit `failed`/`incomplete`** → Erwartung: ready/incomplete-mit-Video öffnet mit Standard; ohne lesbares `screen.mp4` zeigt der Editor den Grund statt zu öffnen. (Task 3 `default_doc_from_meta`, Task 6.)
5. **Export bei vollem Datenträger / Abbruch** → Erwartung: Abbruch löscht die Teil-Datei, Fehler nennt den Grund, Vorschau läuft danach weiter. (Task 4 `export_cancel_leaves_no_file`.)

---

### Task 1: ffmpeg-LGPL-Build + vendorter Compositor als Workspace-Crate

**Files:**
- Create: `scripts/build-ffmpeg-macos.sh`, `vendor/openscreen-compositor/` (Kopie von `crates/compositor` ohne Windows/Linux-Integrationstests), `vendor/openscreen-compositor/{VENDORED.md,RECON.md,LICENSE,THIRD-PARTY-NOTICES.md}`
- Modify: `Cargo.toml` (Workspace-Member), `.gitignore` (`third_party/ffmpeg/`, ffmpeg-Build-Ordner)

**Interfaces (Produces):** Crate `openscreen-compositor` (lib `openscreen_compositor`) baubar mit `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2`; `scripts/build-ffmpeg-macos.sh` legt `third_party/ffmpeg/8.1.2/{include,lib}` an (idempotent: vorhandener Baum mit passender Versionsdatei → nichts tun).

- [ ] **Step 1:** `build.rs` und `shim.c` des Klons lesen (nicht ausführen) und in VENDORED.md festhalten, was sie tun. Skript `build-ffmpeg-macos.sh` schreiben (Download von ffmpeg.org, SHA256 aus `scripts/fetch-ffmpeg-macos.mjs` des Klons übernehmen und prüfen, Flags aus Global Constraints, `make -j`, `make install` in den Zielordner, danach Build-Ordner löschen). Ausführen; `otool -L` einer Dylib zeigt keine GPL-Bibliothek, `ffmpeg -buildconf`-Äquivalent (`grep -c enable-gpl` in `config.h` = 0).
- [ ] **Step 2:** Crate kopieren. Patches (jeder in VENDORED.md mit Grund): bindgen-Ziel aus `CARGO_CFG_TARGET_ARCH`; wgpu/pollster/cosmic-text nur `cfg(target_os = "linux")`; `default = []` (kein segmentation); Tests mit `include_str!` auf Dateien außerhalb der Crate entfernen oder Fixture mitkopieren; Windows-/Linux-Integrationstests weglassen. Workspace: Member eintragen, aber nirgends abhängig (Task 4 hängt es an).
- [ ] **Step 3:** `MAC_FFMPEG_DIR=… CARGO_INCREMENTAL=0 cargo test -p openscreen-compositor --lib` → reine Tests grün (Metal-Tests dürfen laufen; Ausgabe zählen). `cargo check -p inspector-rust-core` unverändert grün.
- [ ] **Step 4:** Commit `build(record): vendor OpenScreen compositor and LGPL ffmpeg build` (ohne `third_party/`).

---

### Task 2: Mikrofon als eigene, gestreckte Tonspur im Compositor

**Files:**
- Modify: `vendor/openscreen-compositor/src/scene.rs` (`SceneClip`), `src/audio_jobs.rs`, `src/audio.rs`, `src/pipeline_macos.rs` (`ClipSource`), `VENDORED.md`

**Interfaces (Produces):** `SceneClip` und `ClipSource` erhalten `extra_audio: Option<ExtraAudio>` mit `ExtraAudio { path: String, offset_sec: f64, gain_db: f64 }` (serde camelCase `extraAudio { path, offsetSec, gainDb }`, Standard `None`). Im Export wird die Quelle `[source_start - offset, source_end - offset]` von `path` dekodiert, mit `gain_db` (geklemmt −60…+12) multipliziert, **vor** dem Strecken mit dem Clip-Ton gemischt — dieselbe Tempo-Strecke wie der Clip.

- [ ] **Step 1: Failing tests** (`audio_jobs.rs` Testmodul, synthetische WAVs im Tempdir, keine GPU): `mic_follows_speed_region` (Clip 0–4 s, Tempo-Bereich 1–3 s ×2, Mikro = Klick bei 3,5 s → im Ergebnis bei 2,5 s ±10 ms) · `mic_gain_applies` (−6 dB → Amplitude ×0,501 ±1 %) · `mic_offset_shifts` (offset 0,25 s) · `missing_extra_audio_is_silence_not_error`.
- [ ] **Step 2–4:** FAIL → implementieren → grün.
- [ ] **Step 5:** Commit `feat(compositor): per-clip extra audio track with gain, stretched with speed`.

---

### Task 3: Editor-Dokument + Szenen-Bau (rein)

**Files:**
- Create: `core/rust-lib/src/record/editor.rs`, `core/rust-lib/src/record/scene_json.rs`, Fixture `core/rust-lib/src/record/fixtures/scene-golden.json`

**Interfaces (Produces):**
- `pub struct EditDoc { version: u32 /*=1*/, clips: Vec<EditClip { start_ms: u64, end_ms: u64 }>, speed: Vec<SpeedRegion { clip: usize, start_ms: u64, end_ms: u64, speed: f64 }>, crop: Vec<Option<CropRect { x: f64, y: f64, w: f64, h: f64 }>> /* je Clip */, output: OutputShape { aspect: Aspect /* source|16:9|9:16|1:1|4:3|3:4|21:9 */ }, mic_gain_db: f64 }` (serde camelCase).
- `pub fn default_doc(meta: &RecordingMeta) -> EditDoc` (ein Clip 0…duration_ms).
- `pub fn load(project: &Project) -> EditDoc` (leer/kaputt/`version` unbekannt → `default_doc`), `pub fn store(project: &mut Project, doc: &EditDoc)`.
- `pub fn normalize(doc: EditDoc, duration_ms: u64) -> EditDoc` — Clips in die Quelle geklemmt, < 100 ms entfernt (mind. einer bleibt), Tempo geklemmt 0,25–16, Bereiche in ihren Clip geklemmt, Überlappungen: spätere Liste gewinnt; Crop in 0…1, min Kante 0,05; `mic_gain_db` −60…+12.
- Programm-Zeit: `pub fn programme_duration_ms(doc) -> u64`, `pub fn programme_to_source(doc, t_ms) -> Option<(usize /*clip*/, u64 /*source ms*/)>`, `pub fn source_to_programme(doc, clip, source_ms) -> u64` — Port der Mathematik von `regions::speed_segments_for_window` (Lücken = 1×).
- `pub fn output_size(meta: &RecordingMeta, aspect: Aspect, target: OutputTarget /* p720|p1080|source */) -> (u32, u32)` — gerade Maße, kurze Seite = 720/1080 bzw. Quelle.
- `pub fn scene_json(project_dir: &Path, project: &Project, doc: &EditDoc, out: (u32,u32), fps: u32) -> serde_json::Value` — erzeugt das Compositor-`Scene`-JSON (Pflichtfelder laut RECON §1e: layout preset `no-webcam`, effects alle aus/0, Hintergrund Volltonfarbe `#000000`, cursor `show:false`, `zoomRegions: []`, `clips` = Dokument-Clips auf `screen.mp4` mit `extraAudio` = `mic.wav` + `offsetSec` aus `RecordingMeta.mic.offset_ms` + `gainDb`, `speedRegions` in Quell-Sekunden mit `clipIndex`, `cropByClip`, `output`).

- [ ] **Step 1: Failing tests:** `default_doc_from_meta` · `load_falls_back_on_garbage` · `normalize_clamps_and_drops_tiny_clips` · `normalize_overlap_later_wins` · `normalize_keeps_one_clip` · `programme_mapping_roundtrip` (Clip 0–10 s, Tempo ×2 auf 2–6 s → Programmdauer 8 s; `programme_to_source(5000) = (0, 7000)`) · `programme_mapping_across_clips` · `output_size_presets` (Quelle 3024×1964: 1080p 16:9 → 1920×1080; 9:16 → 1080×1920; source → 3024×1964) · `scene_json_matches_golden` (Fixture bitgleich, Pfade relativiert) · `scene_json_has_required_fields` (jede Pflicht-Schlüssel-Liste aus RECON §1e vorhanden).
- [ ] **Step 2–4:** FAIL → implementieren → grün.
- [ ] **Step 5:** Commit `feat(record): editor document, programme mapping and scene builder`.

---

### Task 4: Hilfsprogramm `ir-record-render`

**Files:**
- Create: `record-render-protocol/{Cargo.toml,src/lib.rs}` (nur serde/serde_json, kein Plattform-Code — App und Helfer teilen es), `record-render/Cargo.toml` (bin `ir-record-render`, Abhängigkeit `openscreen-compositor` nur `cfg(all(target_os="macos", target_arch="aarch64"))`; auf anderen Zielen `main` meldet „unsupported“ und endet), `record-render/src/{main.rs,session.rs}`
- Modify: `Cargo.toml` (beide Member)

**Interfaces (Produces):** Crate `record-render-protocol`:
- Rahmen: `tag: u8` + `len: u32 LE` + Bytes. Tags: `0x01` JSON-Befehl (App→Helfer), `0x02` JSON-Antwort/Ereignis (Helfer→App), `0x03` Vorschaubild (`gen u64 LE`, `w u32 LE`, `h u32 LE`, RGBA8), `0x04` Vorschau-JPEG (`index u64 LE` + JPEG-Bytes). `pub fn write_frame(w: &mut impl Write, tag: u8, body: &[u8]) -> io::Result<()>`, `pub fn read_frame(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)>` (Obergrenze 256 MiB je Rahmen).
- Befehle (`#[serde(tag = "cmd", rename_all = "snake_case")]`): `Open { scene: Value, preview_w, preview_h }`, `SetScene { scene }`, `SetActiveClip { clip, source_sec }`, `SetPlaying { playing }`, `SetTime { source_sec }`, `Resize { w, h }`, `Frame { since_gen }`, `Thumbs { scene, out_fps, frames: Vec<u64>, max_width }` (→ je Bild ein `0x04`-Rahmen, danach `ThumbsDone`), `Export { scene, kind: Mp4{w,h,fps,codec:"h264"|"hevc"} | Gif{w,h,fps,loop_forever}, out_path }`, `Cancel`, `Shutdown`.
- Ereignisse (`#[serde(tag = "event", rename_all = "snake_case")]`): `Ready`, `ThumbsDone`, `Error { msg }`, `ExportProgress { frames, total }`, `ExportDone { path }`, `ExportFailed { msg, cancelled }`, `Fatal { msg }`.

Umsetzung: Ein Thread besitzt Gpu/Compositor/LiveView (RECON §8: !Sync); stdin-Leser-Thread → Kanal; jede Aufrufkette in `catch_unwind` + `objc::rc::autoreleasepool`; Panic → `Fatal` + sauberes Ende; `eprintln` des Compositors geht nach stderr (App loggt stderr zeilenweise). Export: Vorschau pausieren, `run_composited_multi_cancellable` bzw. `export_gif_cancellable` in eine Staging-Datei, erst bei Erfolg umbenennen; `Cancel` → `ExportControl::cancel`. Ein Compositor je Größe wiederverwenden.

- [ ] **Step 1: Failing tests (rein, `record-render-protocol`):** `frame_roundtrip` · `command_json_roundtrip` (jede Variante) · `truncated_frame_is_error`.
- [ ] **Step 2: Ignorierter Live-Test** `helper_live_export` in `record-render/tests/`: erzeugt mit dem Etappe-1-Live-Pfad nicht möglich → stattdessen nimmt `IR_RECORD_PROJECT=<Projektordner>` (Pflicht, sonst skip), startet den gebauten Helfer, `Open` + 3× `Frame` (nicht leer, Größe stimmt), `Thumbs` 3 Stück, `Export` MP4 1080p30 und GIF 480/15 → Dateien existieren, `ffprobe` Dauer ≈ Programmdauer ±0,2 s; `panic_is_reported_not_fatal` (Befehl `__TestPanic` nur unter `cfg(debug_assertions)`) → `Fatal` empfangen, Prozess endet mit Code ≠ 0, kein Hängen; `export_cancel_leaves_no_file`.
- [ ] **Step 3:** Implementieren; Live-Test ausführen mit einem echten Projekt (das Projekt aus `session_live_display_5s` der Etappe 1 erzeugen: `cargo test … session_live_display_5s -- --ignored`, Ordner aus der Ausgabe), Ausgaben in den Commit-Text.
- [ ] **Step 4:** Commit `feat(record): ir-record-render helper process`.

---

### Task 5: App-Seite — Helfer-Client + IPC

**Files:**
- Create: `core/rust-lib/src/record/render_client.rs`, `core/rust-lib/src/record/editor_ipc.rs`
- Modify: `core/rust-lib/Cargo.toml` (Abhängigkeit `record-render-protocol`), `commands.rs`, `lib.rs`

**Interfaces (Produces):**
- `RenderClient` (ein Helfer pro App, lazy): `spawn()` sucht `ir-record-render` neben der App-Binary (`Contents/MacOS/`), sonst `target/{debug,release}/` (Dev); setzt `DYLD_FALLBACK_LIBRARY_PATH` nur im Dev-Fall; Leser-Thread verteilt Antworten/Bilder; stirbt der Helfer → Zustand `Dead { reason }`, Event `record-editor-fatal`, nächster Befehl startet neu.
- IPC (alle `async`, macOS-gated): `editor_open(dir) -> EditorInit { project: ProjectSummary, meta: RecordingMeta, doc: EditDoc, programme_ms: u64 }` (öffnet Fenster `record-editor` vom Worker-Thread, Helfer `Open`), `editor_update(doc) -> EditDoc` (normalisiert, `SetScene`, speichert entprellt), `editor_seek(programme_ms)`, `editor_play(playing)`, `editor_frame(since_gen) -> tauri::ipc::Response` (Rohbytes: 8 Byte gen + 4 w + 4 h + RGBA; leer wenn nichts Neueres), `editor_resize(w,h)`, `editor_thumbs(count) -> Vec<String /*data:image/jpeg;base64*/>`, `editor_export(opts: ExportOptions { kind: "mp4"|"gif", target, fps, codec, gif_width, loop_forever }) -> ()` mit Events `record-export-progress {frames,total}`, `record-export-done {path}`, `record-export-failed {reason, cancelled}`, `editor_cancel_export()`, `editor_close()`.
- Abspielen: die App rechnet Programm→(Clip, Quelle) mit `editor::programme_to_source` und schickt `SetActiveClip`/`SetTime` an den Clip-Grenzen.

- [ ] **Step 1: Failing tests (rein):** `client_survives_helper_crash` (Fake-Helfer = kleines Testprogramm/`sh -c` das nach `Ready` mit Code 1 endet → `Dead` + nächster `spawn` klappt) · `frame_response_layout` · `export_name_is_unique` (`<Titel> <Datum>.mp4`, Kollision → ` 2`) · `playhead_switches_clip_at_boundary` (rein: Programm-Zeitfolge über eine Clip-Grenze erzeugt genau ein `SetActiveClip`).
- [ ] **Step 2–4:** FAIL → implementieren → grün; `cargo clippy` sauber.
- [ ] **Step 5:** Commit `feat(record): render helper client and editor IPC`.

---

### Task 6: Editor-Fenster

**Files:**
- Create: `core/frontend/src/components/RecordEditor.tsx` (+ Unterkomponenten `editor/Timeline.tsx`, `editor/Preview.tsx`, `editor/CropOverlay.tsx`, `editor/ExportDialog.tsx`), `core/frontend/src/lib/editor.ts` (+ Test), Tests `RecordEditor.test.tsx`
- Modify: `main.tsx` (Route `record-editor`), `ipc.ts`, Capabilities (Label `record-editor`, alle drei), `tauri.conf.json` aller drei (Asset-Scope `$HOME/Movies/Inspector Rust/**` für Ton-Vorschau), Locales (`rec2.ed.*`)

**Interfaces:** `lib/editor.ts` (rein, getestet): `splitAt(doc, programmeMs) -> EditDoc`, `deleteClip(doc, i)`, `setTrim(doc, i, startMs, endMs)`, `addSpeed(doc, clip, startMs, endMs, speed)`, `setCrop(doc, clip, rect | null)`, `cropForPreset(aspect, srcW, srcH) -> CropRect` (zentriert, größtmöglich), `History` (Undo/Redo, max 50, `push/undo/redo/canUndo/canRedo`), `formatTimecode(ms)`; Programm-Mapping ruft NICHT TS-Kopien, sondern verwendet das von `editor_update` zurückgegebene normalisierte Dokument + `programme_ms` (eine Quelle der Wahrheit: Rust).

UI: Kopf (Titel, Speichern-Status, Export-Knopf), Vorschau (Canvas, zieht `editor_frame` je rAF, `ImageData`→`createImageBitmap`; skaliert auf Panelgröße, `editor_resize` entprellt), Transport (Play/Pause Space, ←/→ ±1 Frame, ⇧←/→ ±1 s, Zeitcode), Zeitleiste (Filmstreifen aus `editor_thumbs`, Clips als Blöcke mit Trim-Griffen, Teilen `S`, Löschen `⌫`, Tempo-Bereiche als Band mit Wert 0,25–16 per Ziehen + Menü, Abspielkopf ziehbar), Crop-Modus `C` (Overlay über der Vorschau, Griffe, Preset-Chips), Mikro-Pegel-Regler, Undo/Redo ⌘Z/⇧⌘Z. Ton der Vorschau: zwei `<audio>` (Systemton aus `screen.mp4`, Mikro `mic.wav`) per Asset-Protokoll, an Clip-Grenzen gesprungen, `playbackRate` = Tempo, Mikro-Lautstärke aus `mic_gain_db`. Fehlerzustand (`record-editor-fatal`): Vorschau grau, Grund + „Neu starten“.

- [ ] **Step 1: Failing tests:** `editor.test.ts`: `splitAt` teilt den Clip am Programmzeitpunkt (inkl. Tempo), `History` max 50 + Redo verworfen nach neuem Push, `cropForPreset` 16:9 in 3024×1964 = volle Breite, `formatTimecode` · `RecordEditor.test.tsx` (IPC gemockt): Space ruft `editorPlay(true)`, `S` teilt und ruft `editorUpdate`, ⌘Z stellt zurück, Fatal-Event zeigt Neustart-Knopf.
- [ ] **Step 2–4:** FAIL → implementieren → grün; `pnpm typecheck && pnpm lint`.
- [ ] **Step 5:** Commit `feat(record): editor window with preview, timeline, crop and undo`.

---

### Task 7: Export-Dialog + Einstieg aus Panel und Aufnahme-Ende

**Files:**
- Modify: `editor/ExportDialog.tsx` (aus Task 6, falls dort nur Gerüst), `RecordPanel.tsx` (Enter auf Projekt = Editor öffnen; neue Aufnahme bleibt über den Knopf/`record` ohne Auswahl), `record/session.rs` (nach erfolgreichem Stopp Editor öffnen statt Finder; bei `failed` weiter Finder), Settings `RecordSettingsSection` (Schalter „Nach der Aufnahme Editor öffnen“, Standard an, Key `record.open_editor`), Locales, CommandDoc `record` (Beispiele/Tipps aktualisieren), `features.txt`

**Interfaces:** `ExportDialog` Felder per Global Constraints; Fortschrittsbalken aus `record-export-progress`, Abbrechen → `editor_cancel_export`, Erfolg → Toast + `reveal_path`; Ziel `~/Downloads/<Titel> <YYYY-MM-DD HH.MM>.mp4|gif`.

- [ ] **Step 1: Failing tests:** `RecordPanel.test.tsx` Enter auf ausgewähltem Projekt ruft `editorOpen(dir)`; `ExportDialog` (gemockt) Abbrechen ruft `editorCancelExport`, GIF blendet Codec aus; Rust `after_stop_opens_editor_only_when_ready_and_enabled` (reine Entscheidungsfunktion).
- [ ] **Step 2–4:** FAIL → implementieren → grün; `node scripts/gen-docs.mjs`.
- [ ] **Step 5:** Commit `feat(record): export dialog and editor entry points`.

---

### Task 8: Verpackung, Doku, Version, Ende-zu-Ende

**Files:**
- Modify: `scripts/install-macos.sh` und `scripts/pack-macos-release-dmg.sh` (ffmpeg bauen falls fehlend → `third_party/ffmpeg/8.1.2`; Helfer bauen mit `MAC_FFMPEG_DIR`; Dylibs nach `Contents/Frameworks/` kopieren, `install_name_tool -id @rpath/…` + Abhängigkeiten umschreiben, Helfer `-add_rpath @executable_path/../Frameworks`; Prüfung „keine absoluten Pfade“ per `otool -L`, sonst Abbruch; alles vor dem Signieren), `.github/workflows/*release*` (ffmpeg-Baum cachen, Schlüssel = Version+Flags), `THIRD-PARTY`/About-Hinweise (LGPL-Text + Quell-URL von ffmpeg, OpenScreen MIT, Recordly MIT), `CLAUDE.md` (Abschnitt „record — Etappe 2“: Helfer-Architektur, Protokoll, Fallstricke aus RECON §8), `CHANGELOG.md` (0.229.0, Englisch), Version 0.228.0 → 0.229.0, README/Website-Zeilen, `features.txt`.

- [ ] **Step 1:** `scripts/install-macos.sh` ausführen (Platz prüfen!); `otool -L Contents/MacOS/ir-record-render` zeigt nur `@rpath`/System-Pfade; `codesign --verify --deep --strict` grün; App-Binary linkt KEIN libav (`otool -L … | grep -c libav` = 0).
- [ ] **Step 2:** Ende-zu-Ende (automatisiert, was geht): Live-Test aus Task 4 gegen die installierte Helfer-Binary; Größenmessung Bundle vorher/nachher und Build-Zeit in den Commit-Text.
- [ ] **Step 3:** `cargo test -p inspector-rust-core --lib`, `pnpm test`, `bash scripts/check.sh` grün.
- [ ] **Step 4:** Commit `feat(record): bundle render helper and ffmpeg (v0.229.0)`.

---

## Nach Etappe 2

Etappe 3 (Auto-Zoom-Planer, manuelle Zooms, Zeiger-Rendering, Hintergründe, Rahmen, Schatten, Bewegungsunschärfe) bekommt einen eigenen Plan, wenn Etappe 2 installiert und ausprobiert ist.
