# `record` Etappe 4 — Annotationen, Audio, Tastenkürzel, Presets, Projekte — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Der `record`-Editor bekommt Annotationen (Text mit 5 Schriften und 7 Animationen, Bild, Pfeil in 8 Richtungen, Blur/Mosaik als Rechteck, Oval oder frei gezeichnet), Audiospuren (Musik mit Pegel, Loop, Fades; Voiceover direkt im Editor aufnehmen; Systemton-Pegel; Tonwelle), Clip-Operationen (umordnen, duplizieren, eine andere Aufnahme einfügen), Einrasten, Kopieren/Einfügen, frei belegbare Tastenkürzel, Stil-Presets und eine vollständige Projektliste zum Wiederöffnen — Vorschau und Export gleich.

**Architecture:** Annotationen und Audiospuren sind `Scene`-Felder, die der vendorte Compositor schon liest (`annotations`, `audioTracks`, `audio`); gezeichnet wird in Vorschau und Export vom selben Code. Neu sind das Editor-Dokument v3 in Rust (`record/editor.rs`), die Projektion von Spuren auf Programmzeit (nur in Rust, Regel R12), zwei Vendor-Patches (freie Blur-Maske, Lautheit der Stimme statt des Systemtons) und die Editor-Oberfläche. Voiceover wird nativ aufgenommen (cpal, wie die Aufnahme), nie im Webview.

**Tech Stack:** Rust (core, `record-render`, vendorter Compositor, Metal-Shader), React 19 + TS, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (Etappe 4). Recon mit Zeilenbelegen: `vendor/openscreen-compositor/RECON-stage4.md` (Task 1 legt ihn aus dem Scratchpad `stage4-recon.md` dort ab).

## Global Constraints

- Nur macOS Apple Silicon (wie Etappen 2–3); Linux/Windows/Intel bauen unverändert grün.
- Verhalten 1:1 wie OpenScreen v2.1.2 (`bfd5bc32`): gleiche Konstanten, gleiche Regeln. Zeichnen, Mischen, Ducking und Limiter liegen im Compositor und werden **nicht** neu geschrieben.
- Annotation-Standards (`C/src/components/video-editor/types.ts:425-472`, `C/src/lib/ai-edition/schema/index.ts:397-496`): Position 50 %/50 %, Größe 30 %×20 %; Text Farbe `#ffffff`, Platte neuer Texte `rgba(0, 0, 0, 0.7)`, Schriftgröße 32 [8,200] (an den Compositor `fontSize/1080`), Schrift `Inter`, fett, zentriert, Animation `none`; Schriften genau `Inter, Lora, Oswald, Caveat, IBM Plex Mono` (unbekannt → Inter); Animationen genau `none, fade, rise, pop, slide-left, typewriter, pulse`; Pfeil Richtung `right`, Farbe `#34B27B`, Strich 4, Richtungen `up, down, left, right, up-right, up-left, down-right, down-left`; Blur Stil `mosaic`, Form `rectangle`, Farbe `white`, Stärke 12 [2,40], Blockgröße 12 [4,48], Formen `rectangle, oval, freehand`, freie Form Standard = 9 Punkte; `zIndex` neu = max+1; Text/Bild/Pfeil im Bildraum (`space: "frame"`), Blur immer im Bildschirmraum.
- Audio-Standards (`schema/index.ts:678-705,1490-1536`, `audioTracks.ts:353`, `editorSettings.ts:53-55`): Musik −18 dB, Fades 1000 ms; Voiceover 0 dB, keine Fades; Pegel [−60,+12] in 0,5er-Schritten; Fade [0,5000] ms in 50er-Schritten, länger als die Spur → anteilig gekürzt; Loop ≤ 1000 Wiederholungen; Systemton-Pegel [−12,+12]; Limiter-Decke **−2 dBTP** (Compositor `audio.rs:91`, nicht die −1,5 der Spec).
- Einrasten: 8 px; Ziele = Zeitleisten-Anfang, -Ende, jede Clip-Kante (nicht Abspielkopf, nicht andere Regionen).
- Tonwelle: mono 16 kHz, 200 Blöcke/s, höchstens 24 000 Blöcke, `[min,max]`-Paare als f32; Cache im Projektordner, Schlüssel Größe + mtime.
- Undo-Tiefe 50 bleibt (`lib/editor.ts:27`); jede Ziehgeste, jedes Preset-Anwenden, jedes Auto-Zoom = ein Schritt.
- `EDITOR_VERSION` 2 → **3**; ein v2-Dokument öffnet unverändert (keine Annotationen, keine Spuren, Systemton 0 dB). `PROJECT_SCHEMA` bleibt 1.
- Eigene Dateien (Bilder, Musik, eingefügte Aufnahmen, Voiceover) liegen im Projekt (`<projekt>/assets/`, `<projekt>/takes/<id>/`); Szenenbau nimmt nur einfache Dateinamen darunter (keine Pfadteile), sonst Standard bzw. Weglassen — nie ein Helfer-Fehler.
- Mikrofon nur nativ (`record/mic.rs`), nie `getUserMedia`/`MediaRecorder`.
- Alle UI-Texte en+de; `.claude/rules/animation.md`; kein neues npm-Paket; keine neue Runtime-Crate.
- Laufwerk knapp: `CARGO_INCREMENTAL=0`, `df -h /System/Volumes/Data` vor schweren Builds (Abbruch unter 1,5 GB). Compositor: `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2`.

## Entscheidungen dieses Plans (Annahmen, prüfbar)

- **Tastenkürzel-Standards folgen unseren Konventionen**, wo sie von OpenScreen abweichen: **S = Teilen**, **C = Zuschneiden** (OpenScreen: S = Tempo-Region, C = Kamera-Layout). Alles ist frei belegbar; Upstreams T (Trim-Region) entfällt, weil unser Schnitt per Teilen + Löschen dasselbe leistet. **⌘D = Duplizieren** (OpenScreen: ⌘D = Löschen); Löschen bleibt ⌫/Entf.
- **Freie Blur-Form wird echt gezeichnet** (Vendor-Patch 13): Upstream bietet sie an, zeichnet aber nur das umschließende Rechteck — das wäre ein falsches Versprechen.
- **Lautheit:** Der Export normalisiert heute den Systemton auf −16 LUFS und das Mikrofon gar nicht — umgekehrt zu dem, was eine Demo braucht. Patch 14 normalisiert die Stimme (Mikrofon/Voiceover) und lässt den Systemton auf seinem Pegel.
- **Eine andere Aufnahme einfügen = Kopie ins Projekt** (`<projekt>/takes/<id>/`, APFS-Klon per `cp -c`), damit ein Projekt beim Verschieben vollständig bleibt.
- **Presets sind Dateien** (`<Projektordner>/Presets/*.irpreset`, teilbar), Format `{format:"inspector-rust-record-preset", version:1, name, appearance, cursor, aspect}`; eigene Bilder/Zeiger werden neben dem Preset abgelegt (`Presets/assets/`). Das Seitenverhältnis wirkt beim Anwenden, nie bei neuen Projekten (wie Upstream).
- **Spec-Korrekturen** im selben Release: VP9 entfällt (Upstream hat den Encoder entfernt), Limiter −2 dBTP.

## Review Focus

1. **Spur oder Annotation über einen Schnitt / gelöschten Clip / umgeordnete Clips** → bleibt am Inhalt ihres Clips (Quellzeit), wandert beim Umordnen mit, verschwindet mit dem Clip, nie außerhalb. (Task 2 `reorder_moves_annotations_and_tracks`, Task 1 `normalize_drops_annotation_of_deleted_clip`.)
2. **Musikdatei kürzer als das Video mit Loop / länger ohne Loop / fehlt nach dem Verschieben** → Loop füllt bis Programmende (≤ 1000), ohne Loop endet sie, fehlende Datei fällt aus der Szene. (Task 4 `music_loop_fills_programme`, `missing_music_is_skipped`.)
3. **Voiceover während Wiedergabe bis über das Programmende / Abbruch mit Esc / Mikrofon fehlt** → Aufnahme endet am Programmende, Abbruch verwirft die Datei, fehlendes Mikrofon = Hinweis, kein halber Eintrag. (Task 5 `voiceover_stops_at_programme_end`, `voiceover_cancel_discards`.)
4. **Tastenkürzel doppelt belegt / Kürzel während Texteingabe in einer Annotation** → Konflikt wird angeboten zu tauschen, feste Kürzel sind gesperrt; in Textfeldern löst kein Kürzel aus. (Task 9 `conflict_offers_swap`, `typing_in_field_triggers_nothing`.)
5. **Preset mit eigenem Hintergrundbild auf ein anderes Projekt anwenden / Preset-Datei von Hand kaputt** → Bild wird ins Projekt kopiert; kaputte Datei erscheint mit Hinweis, wendet nichts an. (Task 10 `apply_copies_preset_assets`, `broken_preset_is_listed_not_applied`.)

---

### Task 1: Editor-Dokument v3 (Annotationen, Audiospuren, Quellen) + Migration

**Files:**
- Modify: `core/rust-lib/src/record/editor.rs`, `core/frontend/src/lib/ipc.ts`, `core/frontend/src/lib/editor.ts`
- Create: `vendor/openscreen-compositor/RECON-stage4.md` (Kopie von `<scratchpad>/stage4-recon.md`)
- Test: `editor.rs`-Testmodul, `core/frontend/src/lib/editor.test.ts`

**Interfaces (Produces)** — `serde(rename_all="camelCase")`, TS spiegelt 1:1:
- `EDITOR_VERSION: u32 = 3`.
- `EditClip` erhält `#[serde(default)] source: Option<String>` (`None` = Hauptaufnahme des Projekts; `Some(id)` = `<projekt>/takes/<id>/`).
- `EditDoc` erhält `#[serde(default)] annotations: Vec<Annotation>`, `#[serde(default)] audio_tracks: Vec<AudioTrack>`, `#[serde(default)] system_gain_db: f64`.
- `Annotation { id: String, clip: usize, start_ms: u64, end_ms: u64 /*SOURCE ms*/, x: f64, y: f64, w: f64, h: f64 /*Prozent 0..100*/, z_index: i32, body: AnnotationBody }`.
- `AnnotationBody` (`#[serde(tag="kind", rename_all="lowercase")]`): `Text(TextBody)` · `Image { file: String }` · `Arrow { direction: ArrowDirection, color: String, stroke_width: f64 }` · `Blur(BlurBody)`.
- `TextBody { content, color, background, font_size: f64, font_family: String, bold: bool, italic: bool, underline: bool, align: TextAlign /*left|center|right*/, animation: TextAnimation }`; `TextAnimation` kebab `none|fade|rise|pop|slide-left|typewriter|pulse`; `ArrowDirection` kebab (8 Werte aus Global Constraints).
- `BlurBody { style: BlurStyle /*blur|mosaic*/, shape: BlurShape /*rectangle|oval|freehand*/, color: BlurTint /*white|black*/, intensity: f64, block_size: f64, points: Vec<(f64, f64)> /*Prozent im Annotationsrechteck*/ }`.
- `AudioTrack { id, kind: TrackKind /*music|voiceover*/, file: String /*Name unter assets*/, clip: usize, start_ms: u64 /*SOURCE ms im Anker-Clip*/, duration_ms: u64 /*Länge der Datei*/, offset_ms: u64, span_ms: Option<u64> /*Voiceover: aufgenommene Länge*/, gain_db: f64, looped: bool, fade_in_ms: u64, fade_out_ms: u64, muted: bool, label: String }`.
- Konstanten: `TEXT_FONTS: [&str; 5]`, `ANNOTATION_DEFAULT_RECT = (50.,50.,30.,20.)`, `FONT_SIZE_RANGE = (8.,200.)`, `BLUR_INTENSITY_RANGE = (2.,40.)`, `BLUR_BLOCK_RANGE = (4.,48.)`, `TRACK_GAIN_RANGE = (-60.,12.)`, `FADE_MAX_MS = 5000`, `SYSTEM_GAIN_RANGE = (-12.,12.)`, `MUSIC_DEFAULT_GAIN_DB = -18.`, `MUSIC_DEFAULT_FADE_MS = 1000`.
- `migrate`: v2 → v3 = alles neu leer/0, sonst unverändert; v1 läuft über v2.
- `normalize` zusätzlich: Annotationen und Spuren (Anker-Clip) wie Zooms — auf ihren Clip klemmen, Split ordnet dem Teil mit dem Startpunkt zu, gelöschter Clip = weg, Indizes nachziehen; Zahlen/Farben auf Bereiche klemmen; unbekannte Schrift → Inter; freie Form mit < 3 Punkten → 9-Punkte-Standard; Spuren: Fades anteilig kürzen wenn länger als die Spur.
- TS: `addAnnotation(doc, kind, clip, sourceMs)`, `updateAnnotation(doc, id, patch)`, `removeAnnotation`, `addTrack(doc, kind, file, durationMs, clip, sourceMs)`, `updateTrack`, `removeTrack`, `setSystemGain`; Konstanten gespiegelt, Pin liest `editor.rs` von Platte.

- [ ] **Step 1: Failing tests (Rust):** `v2_doc_migrates_without_change` · `new_text_annotation_defaults` (50/50/30/20, `#ffffff`, Platte `rgba(0, 0, 0, 0.7)`, 32, Inter, fett, center, none, z = max+1) · `normalize_drops_annotation_of_deleted_clip` · `normalize_annotation_follows_split` · `normalize_clamps_blur_and_font` (Stärke 99 → 40, Block 1 → 4, Größe 300 → 200, Schrift „Comic“ → Inter) · `freehand_with_two_points_gets_default` · `track_fades_scaled_to_span` · `serde_roundtrip_v3`.
- [ ] **Step 2: Failing tests (TS):** Operationen + Konstanten-Pin.
- [ ] **Step 3:** Implementieren; bestehende Tests auf v3, ohne ihre Aussage zu ändern.
- [ ] **Step 4:** `cargo test -p inspector-rust-core --lib record::editor` + Vitest grün. Commit `feat(record): editor document v3 with annotations and audio tracks`.

---

### Task 2: Clip-Operationen + mehrere Quellen (umordnen, duplizieren, Aufnahme einfügen)

**Files:**
- Modify: `core/rust-lib/src/record/editor.rs`, `core/rust-lib/src/record/scene_json.rs` (Pfade je Clip), `core/rust-lib/src/record/editor_ipc.rs`, `core/rust-lib/src/record/session.rs` (Take-Import), `core/rust-lib/src/commands.rs` + `lib.rs`, `core/frontend/src/lib/ipc.ts`, `core/frontend/src/components/editor/Timeline.tsx`, `record-render/src/session.rs` (falls die LiveView beim Clipwechsel eine andere Quelle braucht), Locales
- Test: `editor.rs`, `editor_ipc.rs`, `scene_json.rs`, `Timeline.test.tsx`, `record-render/tests/helper_live.rs`

**Interfaces:**
- Consumes: Task 1 (`EditClip.source`, Annotation/Track-Anker).
- Produces (Rust, rein): `fn move_clip(doc: &EditDoc, from: usize, to: usize) -> EditDoc`, `fn duplicate_clip(doc: &EditDoc, index: usize) -> EditDoc` (Kopie direkt dahinter, mit eigenen Zooms/Annotationen/Spuren unter neuen ids), `fn append_take(doc: &EditDoc, take_id: &str, meta: &TakeMeta) -> EditDoc` (ein Clip über die ganze Aufnahme, ans Ende); jede Operation zieht `speed`, `crop`, `zooms`, `annotations`, `audio_tracks` mit.
- `TakeMeta { duration_ms: u64, width: u32, height: u32, system_audio: bool, has_mic: bool, has_cursor: bool }`; `fn take_dir(project: &Path, source: Option<&str>) -> PathBuf`.
- IPC (async + `spawn_blocking`): `editor_import_take(session_id: u64, project_dir: String) -> EditorView` — prüft `is_project_dir`, `ready`-Status, kopiert `screen.mp4`, `mic.wav`, `cursor.json`, `project.json` per `cp -c` nach `<projekt>/takes/<uuid>/`, hängt an, normalisiert, speichert; Fehler als Sentinels `record.editor.take_*`.
- Szene: jeder `SceneClip` bekommt `screenPath`, `cursorPath`, `extraAudio`, `hasAudio` seiner Quelle.
- UI: Clips in der Zeitleiste ziehen = umordnen (Einfügemarke zwischen Clips); Kontextmenü/Knopf „Duplizieren“ (⌘D) und „Aufnahme einfügen…“ (Liste der anderen Projekte aus `record_list_projects`).

- [ ] **Step 1: Failing tests:** `reorder_moves_annotations_and_tracks` (Clip 0 ↔ 2: alle an Clip 0 hängenden Dinge zeigen auf den neuen Index) · `duplicate_copies_regions_with_new_ids` · `append_take_adds_clip_at_end` · `scene_clip_paths_follow_source` (Clip mit `source: Some("t1")` → `screenPath` unter `takes/t1/`) · `import_take_rejects_non_project` · `import_take_rejects_failed_project` · TS: Ziehen über zwei Clips = `moveClip` mit richtigem Ziel.
- [ ] **Step 2: Live-Test** (`#[ignore]`, `IR_RECORD_PROJECT` + `IR_RECORD_PROJECT2`): Szene mit zwei Quellen, Vorschau springt über die Clipgrenze, Standbild im zweiten Clip zeigt die zweite Aufnahme (mittlere Abweichung zum Export-Standbild < 3, schlimmster 32-px-Block < 20). Ergibt sich, dass die LiveView beim Clipwechsel die Quelle nicht neu lädt: Vendor-Patch (nächste freie Nummer, in `VENDORED.md`) statt Umweg.
- [ ] **Step 3:** Implementieren; Ergebnisse + Messwerte in den Bericht.
- [ ] **Step 4:** Tests grün. Commit `feat(record): reorder, duplicate and insert other takes`.

---

### Task 3: Annotationen in der Szene + Schriften + freie Blur-Form (Patch 13)

**Files:**
- Create: `core/rust-lib/assets/record/fonts/{Inter,Lora,Oswald,Caveat,IBMPlexMono}-{Regular,Bold}.ttf` + `*-OFL.txt` (aus `C/public/fonts/`, je Datei vorher mit `file` geprüft)
- Modify: `core/rust-lib/src/record/scene_json.rs`, `core/rust-lib/src/record/assets.rs` (`fonts_dir()`), `core/rust-lib/src/record/render_client.rs` (setzt `OPENSCREEN_FONTS_DIR` beim Start des Helfers), `scripts/bundle-record-render.sh` (Schriften nach `Contents/Resources/record/fonts`, OFL-Texte nach `THIRD-PARTY/fonts/`), `vendor/openscreen-compositor/src/{compositor_macos.rs,shaders.metal,frame_geometry.rs}` (Patch 13), `vendor/openscreen-compositor/VENDORED.md`, `vendor/openscreen-compositor/THIRD-PARTY-NOTICES.md`
- Test: `scene_json.rs`, Compositor-Tests, `record-render/tests/helper_live.rs`

**Interfaces:**
- Consumes: Task 1 `Annotation`.
- Produces: `scene_json` gibt `annotations` aus, Felder genau wie `V/src/scene.rs:473-592` (`clipIndex`, `startSec`/`endSec` in Quellsekunden, `kind` `text|image|figure|blur`, `space`, `x,y,w,h` 0..1, `zIndex`, `text{content,color,backgroundColor,fontSizeRel,fontFamily,fontWeight,fontStyle,textDecoration,textAlign,animation}`, `imagePath` absolut, `figure{direction,color,strokeWidth}`, `blur{style,shape,color,intensity,blockSize,freehandPoints}`), sortiert aufsteigend nach `zIndex`; Bild nur bei einfachem Dateinamen unter `assets/`, sonst weggelassen.
- Patch 13: `freehandPoints` als **Prozent im Annotationsrechteck** (unsere Festlegung; die Upstream-Einheit ist widersprüchlich, siehe Recon §1d) wird als geschlossenes Polygon maskiert — außerhalb des Polygons kein Blur/Mosaik, Kante geglättet wie das Oval; folgt Zoom wie die anderen Formen.
- `AssetRoot::fonts_dir() -> PathBuf`; der Helfer bekommt die Variable **vor** dem ersten Textzeichnen (die Schriften sind im Compositor per `OnceLock` gecacht).

- [ ] **Step 1: Failing tests:** `annotations_emitted_in_scene` (alle vier Arten, Felder + Einheiten: Prozent → 0..1, `fontSize 32` → `fontSizeRel 32/1080`, Ms → Sekunden) · `annotations_sorted_by_z` · `image_outside_assets_dropped` (`../x.png`, absolut) · `fonts_dir_has_all_ten` · Compositor `freehand_mask_excludes_outside_polygon` (Dreieck: Pixel außerhalb unverändert, innen gemischt) · `freehand_mask_rectangle_unchanged` (Rechteck/Oval bitgleich wie vorher). Die zwei bisher ignorierten Schrift-Tests aus Patch 7 laufen wieder.
- [ ] **Step 2: Live-Test** (`#[ignore]`): Szene mit Text in Caveat, Pfeil, Mosaik-Oval, freier Form — Vorschau vs. Export-Standbild (mittlere Abweichung < 3, schlimmster Block < 20); Standbild ansehen und im Bericht beschreiben.
- [ ] **Step 3:** Implementieren, Patch 13 in `VENDORED.md` beschreiben, Schrift-Lizenzen in den Notices.
- [ ] **Step 4:** Tests grün. Commit `feat(record): annotations with fonts and freehand blur`.

---

### Task 4: Audio in der Szene (Spuren, Systemton, Lautheit, Patch 14, Tonwelle)

**Files:**
- Create: `core/rust-lib/src/record/audio_tracks.rs` (Projektion auf Programmzeit), `core/rust-lib/src/record/peaks.rs`
- Modify: `core/rust-lib/src/record/scene_json.rs`, `core/rust-lib/src/record/editor.rs` (`EditorView.tracks`, `EditorView.waveforms`), `core/rust-lib/src/record/editor_ipc.rs`, `record-render-protocol` + `record-render/src/session.rs` (Befehl `Peaks`), `vendor/openscreen-compositor/src/{audio_jobs.rs,audio.rs}` (Patch 14), `VENDORED.md`
- Test: `audio_tracks.rs`, `peaks.rs`, `scene_json.rs`, Compositor-Tests

**Interfaces:**
- Consumes: Task 1 `AudioTrack`, `system_gain_db`; Task 2 Quellen.
- Produces: `fn project_tracks(doc: &EditDoc, project: &Path) -> Vec<SceneTrack>` — 1:1-Port von `C/src/native/sceneDescription.ts:915-1047`: jede Aufnahme einmal ab ihrem Kopf-Fragment; Voiceover ohne Loop wird um Schnitte herum geteilt (Wörter fallen mit dem Video weg); Musik ohne Loop = ein Eintrag; Loop = ein Eintrag je Wiederholung (≤ 1000), Fades nur an den äußeren Kanten; Tempo verschiebt nur den Start, streckt nie; `trimEndSec` immer gesetzt; stummgeschaltete und fehlende Dateien entfallen. `SceneTrack` = Felder von `V/src/scene.rs:833-867` (`path, startSec /*Programmzeit*/, gainDb, trimStartSec, trimEndSec, fadeInSec, fadeOutSec, kind`).
- `scene_json` gibt `audio: {gainDb: system_gain_db}` und `audioTracks` aus.
- Patch 14 (`decode_and_stretch_clip_audio_with_extra`, `V/src/audio_jobs.rs:86-140`): neue Option `normalise: Voice|Screen` an `SceneClip` (`#[serde(default)]` = bisheriges Verhalten `Screen`); wir senden `Voice` → `extraAudio` (Mikrofon) wird auf −16 LUFS (≤ +12 dB) gebracht, der Systemton nicht; Musik-/Voiceover-Spuren gehen durch den Mono-auf-Einheit-Pfad aus Patch 9.
- Tonwelle: Protokollbefehl `Peaks { path: String, cache: String }` → Antwort `Event::Peaks { blocks: u32 }` + Datei `cache` (`[min,max]` f32, 200/s, ≤ 24 000, mono 16 kHz, über `audio::decode_clip_audio`); `mic.wav` liest die App direkt mit `wav.rs`. `fn peaks_cache_name(path, size, mtime) -> String` (`peaks-<hash>.f32`). `EditorView.waveforms: Vec<WaveSpan>` = Spitzen bereits auf Programmzeit projiziert (R12).

- [ ] **Step 1: Failing tests:** `music_loop_fills_programme` (10 s Musik, 35 s Programm → 4 Einträge, Fade-in nur am ersten, Fade-out nur am letzten) · `music_without_loop_single_entry` · `voiceover_split_around_cut` · `speed_shifts_track_start_only` · `missing_music_is_skipped` · `muted_track_skipped` · `trim_end_always_set` · `loop_capped_at_1000` · `system_gain_in_scene` · `peaks_block_count_and_cap` · `peaks_cache_name_changes_with_mtime` · Compositor `voice_normalised_screen_untouched` (leiser Mikrofon-Sinus wird angehoben, lauter Systemton nicht).
- [ ] **Step 2: Live-Test** (`#[ignore]`): Export mit Musik (Loop, −18 dB, Fades) und Voiceover-WAV; Pegelverlauf des Exports (ffmpeg `ebur128` oder `astats`) im Bericht: Musik hörbar, unter der Stimme abgesenkt, True Peak ≤ −2 dBTP.
- [ ] **Step 3:** Implementieren, Patch 14 beschreiben.
- [ ] **Step 4:** Tests grün. Commit `feat(record): music, voiceover tracks, system gain and waveforms`.

---

### Task 5: Voiceover nativ aufnehmen

**Files:**
- Create: `core/rust-lib/src/record/voiceover.rs`
- Modify: `core/rust-lib/src/record/editor_ipc.rs`, `core/rust-lib/src/commands.rs` + `lib.rs`, `core/frontend/src/lib/ipc.ts`
- Test: `voiceover.rs`, `editor_ipc.rs`

**Interfaces:**
- Consumes: `record/mic.rs` (`MicStream`), `record/wav.rs` (`WavSink`), Task 1 `addTrack`.
- Produces IPC: `editor_voiceover_start(session_id, programme_ms: u64) -> VoiceoverState` (nimmt nach `<projekt>/assets/voiceover-<ts>.wav` auf; startet die Wiedergabe der Vorschau ab `programme_ms`), `editor_voiceover_stop(session_id) -> EditorView` (Spur `kind: voiceover`, 0 dB, keine Fades, `span_ms` = aufgenommene Länge, Anker = Clip/Quellzeit von `programme_ms`), `editor_voiceover_cancel(session_id)` (Datei löschen, kein Eintrag); Event `record-voiceover-level { rms }` ~10/s.
- Verhalten: Aufnahme endet spätestens am Programmende; keine Aufnahme, wenn schon eine läuft (`record.editor.voiceover_busy`); fehlendes Mikrofon = `record.editor.no_mic`; Mikrofon-Freigabe fehlt = Sentinel mit Hinweis.

- [ ] **Step 1: Failing tests:** `voiceover_stops_at_programme_end` (Stoppzeit = min(Stop, Ende)) · `voiceover_cancel_discards` · `voiceover_anchor_from_programme_ms` (über Task-2-Abbildung) · `second_start_is_busy`. Der eigentliche Mikrofonpfad hat einen `#[ignore]`-Live-Test (3 s Aufnahme → WAV-Länge ≈ 3 s).
- [ ] **Step 2:** Implementieren (Mikrofon wie die Aufnahme; nie im Webview).
- [ ] **Step 3:** Tests grün; Live-Test laufen lassen, Ergebnis in den Bericht. Commit `feat(record): record a voiceover in the editor`.

---

### Task 6: Annotationen bearbeiten (Spur, Inspector, Vorschau-Griffe)

**Files:**
- Create: `core/frontend/src/components/editor/AnnotationOverlay.tsx`, `core/frontend/src/components/editor/AnnotationTab.tsx`, Tests dazu
- Modify: `core/frontend/src/components/editor/{Timeline.tsx,Inspector.tsx,Preview.tsx}`, `core/frontend/src/components/RecordEditor.tsx`, `core/rust-lib/src/record/editor.rs` (`EditorView.annotation_spans` in Programmzeit), `core/rust-lib/src/record/editor_ipc.rs` (`editor_import_asset` nimmt auch Bilder für Annotationen), `core/frontend/src/styles.css` (`@font-face` der 10 Schriften aus dem Asset-Scope), Locales, `core/frontend/src/mockup/backend.ts`
- Test: `AnnotationOverlay.test.tsx`, `AnnotationTab.test.tsx`, `Timeline.test.tsx`, `editor.rs`

**Interfaces:**
- Consumes: Task 1 TS-Operationen, Task 3 Szene; `EditorView.screen_rects`/`outputSize` (Etappe 3).
- Produces: Annotationsspur über der Zoom-Spur (Blöcke aus `annotation_spans`, Kennfarbe je Art); Reiter **Annotation** im Inspector für die gewählte Annotation: Text (Inhalt, Schrift mit echter Schriftvorschau, Größe, Farbe, Platte, fett/kursiv/unterstrichen, Ausrichtung, Animation), Bild (Datei wählen), Pfeil (8 Richtungen als Raster, Farbe, Strich), Blur (Stil, Form, Farbe, Stärke, Blockgröße; bei „frei“ Punkte in der Vorschau setzen/ziehen); in der Vorschau: Rahmen + Ziehen + 8 Größengriffe für die gewählte Annotation (Bildraum über `outputSize`, Blur über `screen_rects`; bei aktivem Zoom bleiben Blur-Griffe aus und ein Hinweis sagt warum); Tab/⇧Tab wechselt nach Startzeit.
- Regeln: Texteingabe in Annotationen löst keine Editor-Kürzel aus; Schieberegler = ein Undo-Schritt (`LiveGesture`).

- [ ] **Step 1: Failing tests:** Griff ziehen ändert x/y/w/h in Prozent und klemmt an den Rand · Größengriff an der Ecke hält die Gegenecke · Tab-Reihenfolge nach Startzeit · Tippen im Textfeld löst kein `S` aus · Spur-Blöcke aus `annotation_spans` (nicht in TS gerechnet) · `annotation_spans_in_programme_time` (Rust) · freie Form: Klick fügt Punkt ein, Ziehen verschiebt, Entf löscht (min. 3 bleiben).
- [ ] **Step 2:** Implementieren; Mockup mit erfundenen Daten ansehen (alle vier Arten, Inspector-Reiter), Befund in den Bericht.
- [ ] **Step 3:** Vitest + Typecheck + Lint grün. Commit `feat(record): annotation track, inspector and preview handles`.

---

### Task 7: Audio bearbeiten (Spur, Tonwelle, Musik, Voiceover-Knopf, Vorschau-Mischung)

**Files:**
- Create: `core/frontend/src/components/editor/AudioLane.tsx`, `core/frontend/src/components/editor/AudioTab.tsx`, `core/frontend/src/lib/audio-mix.ts`, Tests dazu
- Modify: `core/frontend/src/components/editor/{Timeline.tsx,Inspector.tsx,AudioPreview.tsx}`, `core/frontend/src/components/RecordEditor.tsx`, `core/rust-lib/src/record/editor_ipc.rs` (`editor_import_asset` nimmt `audio`: mp3/m4a/aac/wav/flac, ≤ 200 MB, liest die Dauer über den Helfer), Locales, Mockup
- Test: `AudioLane.test.tsx`, `AudioTab.test.tsx`, `audio-mix.test.ts`

**Interfaces:**
- Consumes: Task 4 `EditorView.tracks`/`waveforms`, Task 5 IPC, Task 1 TS-Operationen.
- Produces: Audiospur unter den Clips (ein Balken je Eintrag, Tonwelle, Loop-Wiederholungen angedeutet; ziehen = verschieben, Kanten = `offset_ms`/Länge); Tonwelle auch in den Clips (Mikrofon + Systemton); Reiter **Audio**: Systemton-Pegel ±12, Mikrofon-Pegel (bestehend), je Spur Pegel, Loop, Fade-in/-out, stumm, Name, löschen; Knöpfe „Musik hinzufügen…“ (M) und „Voiceover aufnehmen“ (V; zeigt Pegel, Stopp, Esc = verwerfen; spielt die Vorschau mit gesenkten anderen Spuren).
- Vorschau-Mischung (`lib/audio-mix.ts`, rein): `gainAt(track, programmeMs) -> number` (Pegel × Fade-Hüllkurve), `limiterCurve(ceilingDb = -2) -> Float32Array` (WaveShaper); `AudioPreview` mischt über WebAudio (Ausgabe nur — Mikrofon nie im Webview), Spuren nach Programmzeit gesetzt aus `EditorView.tracks` (keine TS-Projektion), Pegel > 0 dB hörbar.

- [ ] **Step 1: Failing tests:** `gainAt` (Fade-in/-out, Loop-Kanten, stumm = 0) · `limiterCurve` begrenzt bei −2 dBTP · Spur-Balken aus `EditorView.tracks` · Ziehen klemmt an Programmanfang/-ende und wird geklemmt (Einrasten kommt in Task 8 dazu) · Voiceover-Knopf: Start → Stopp ruft die IPCs in Reihenfolge, Esc ruft `cancel` · M öffnet den Dateidialog.
- [ ] **Step 2:** Implementieren; Mockup ansehen.
- [ ] **Step 3:** Vitest + Typecheck + Lint grün. Commit `feat(record): audio lane, waveforms and preview mix`.

---

### Task 8: Kopieren/Einfügen, Einrasten, Auswahl

**Files:**
- Create: `core/frontend/src/lib/timeline-snap.ts`, `core/frontend/src/lib/clipboard-regions.ts`, Tests dazu
- Modify: `core/frontend/src/components/editor/Timeline.tsx`, `AnnotationOverlay.tsx`, `AudioLane.tsx`, `RecordEditor.tsx`, `lib/editor.ts` (`moveZoomProg`/`trimZoomProg` und die neuen Zieh-Funktionen rasten ein)
- Test: `timeline-snap.test.ts`, `clipboard-regions.test.ts`, `Timeline.test.tsx`

**Interfaces:**
- Produces: `snap(ms: number, targets: number[], pxPerMs: number, thresholdPx = 8) -> { ms: number, target: number | null }`; `snapTargets(view: RecordEditorView) -> number[]` (0, Programmende, jede Clipkante — aus `view.clips`); Führungslinie in der Zeitleiste solange eingerastet; `copyRegions(doc, selection) -> ClipboardRegions` / `pasteRegions(doc, clip, sourceMs, data) -> doc` für Zooms, Annotationen, Spuren (neue ids, am Abspielkopf, im Clip geklemmt); Auswahl = ein Objekt (Clip, Zoom, Annotation oder Spur) mit gemeinsamem `Selection`-Typ.

- [ ] **Step 1: Failing tests:** `snap` innerhalb 8 px rastet, 9 px nicht; Abspielkopf ist kein Ziel · Kopieren + Einfügen einer Annotation ergibt neue id am Abspielkopf · Einfügen über das Clipende hinaus wird gekürzt · ⌘X = Kopieren + Löschen in einem Undo-Schritt.
- [ ] **Step 2:** Implementieren.
- [ ] **Step 3:** Vitest + Typecheck + Lint grün. Commit `feat(record): snapping, copy and paste of regions`.

---

### Task 9: Frei belegbare Tastenkürzel

**Files:**
- Create: `core/frontend/src/lib/editor-shortcuts.ts`, `core/frontend/src/components/editor/ShortcutsDialog.tsx`, Tests dazu
- Modify: `core/rust-lib/src/record/session.rs` (Einstellung `record.shortcuts`, JSON), `core/rust-lib/src/commands.rs` + `lib.rs` (`get_/set_record_shortcuts`), `RecordEditor.tsx`, `Timeline.tsx`, `AnnotationOverlay.tsx`, `AudioLane.tsx`, `RecordSettingsSection.tsx`, Locales
- Test: `editor-shortcuts.test.ts`, `ShortcutsDialog.test.tsx`, `session.rs`

**Interfaces:**
- Produces: `type Binding = { key: string; ctrl?: boolean; shift?: boolean; alt?: boolean }` (`ctrl` = ⌘ auf dem Mac), `type ActionId = "playPause"|"split"|"crop"|"addZoom"|"addAnnotation"|"addAudio"|"addVoiceover"|"duplicate"|"deleteSelected"|"copy"|"cut"|"paste"|"openProject"|"showShortcuts"`; `DEFAULT_BINDINGS`: playPause Space, split S, crop C, addZoom Z, addAnnotation A, addAudio M, addVoiceover V, duplicate ⌘D, deleteSelected ⌫ (Entf gleichwertig), copy ⌘C, cut ⌘X, paste ⌘V, openProject ⌘O, showShortcuts `?`; `FIXED`: ⌘Z, ⌘⇧Z, ←/→ (⇧ = 1 s), Tab/⇧Tab, Esc (nicht belegbar). `matchAction(e: KeyboardEvent, bindings) -> ActionId | null` (ignoriert Eingaben in Textfeldern/contentEditable), `findConflict(bindings, action, binding) -> { kind: "fixed" } | { kind: "action", action } | null`, `mergeWithDefaults(stored) -> Bindings`.
- Speicher: Einstellung `record.shortcuts` (nur Abweichungen), Rust-Seite liest/schreibt das JSON unverändert und gibt bei Unlesbarem `{}` zurück.
- Dialog: Liste aller Aktionen mit Kürzel, Klick = neue Taste aufnehmen, Konflikt mit fester Taste = gesperrt mit Hinweis, mit anderer Aktion = „Tauschen“/„Abbrechen“, „Alle zurücksetzen“; erreichbar mit `?` im Editor und aus Settings → Aufnahme. Leere Spuren zeigen ihr Kürzel als Hinweis.
- **Alle** Editor-Tastenhandler gehen über `matchAction`; die `defaultPrevented`-Reihenfolge aus Etappe 3 (Timeline zuerst) bleibt.

- [ ] **Step 1: Failing tests:** Standardbelegung (Pin der Tabelle oben) · `conflict_offers_swap` · `fixed_key_refused` · `typing_in_field_triggers_nothing` · `merge_keeps_defaults_for_missing` · umbelegtes `split` auf `X` → `S` tut nichts mehr, `X` teilt · Rust `unreadable_shortcuts_fall_back_to_empty`.
- [ ] **Step 2:** Implementieren; alle bisher fest verdrahteten Kürzel umstellen.
- [ ] **Step 3:** Tests grün. Commit `feat(record): configurable editor shortcuts`.

---

### Task 10: Stil-Presets

**Files:**
- Create: `core/rust-lib/src/record/presets.rs`, `core/frontend/src/components/editor/PresetsMenu.tsx`, Tests dazu
- Modify: `core/rust-lib/src/record/editor.rs` (`default_doc` nimmt ein optionales Preset), `core/rust-lib/src/record/editor_ipc.rs`, `core/rust-lib/src/record/session.rs` (Einstellung `record.default_preset`), `commands.rs` + `lib.rs`, `ipc.ts`, `RecordEditor.tsx`, Locales, Mockup
- Test: `presets.rs`, `PresetsMenu.test.tsx`

**Interfaces:**
- Produces: `Preset { format: String /*"inspector-rust-record-preset"*/, version: u32 /*1*/, name: String /*≤ 80*/, appearance: Appearance, cursor: CursorStyle, aspect: Aspect }`; Ablage `<Projektordner>/Presets/<bereinigter Name>.irpreset` (2-Leerzeichen-JSON + Zeilenumbruch, atomar geschrieben), eigene Bilder/Zeiger unter `Presets/assets/<inhalts-hash>.<ext>`.
- `fn list_presets(root) -> Vec<PresetEntry { name, file, ok: bool, error: Option<String> }>` (kaputte Dateien stehen drin, `ok: false`), `fn save_preset(root, name, doc, project) -> Result<PresetEntry>` (Namen ohne Groß/klein-Unterschied eindeutig; eigene Dateien kopieren), `fn apply_preset(doc, preset, project) -> EditDoc` (eigene Dateien ins Projekt kopieren, fehlend → Standard), `rename_preset`, `delete_preset`.
- IPC (async + `spawn_blocking`): `record_presets_list`, `record_preset_save(session_id, name)`, `record_preset_apply(session_id, file) -> EditorView`, `record_preset_rename`, `record_preset_delete`, `record_preset_set_default(file | null)`, `record_presets_reveal`.
- Neue Projekte nehmen `record.default_preset` (ohne Seitenverhältnis), sonst die Standards aus Etappe 3. Anwenden = ein Undo-Schritt. Version ≠ 1 oder falsches `format` = `ok: false`.

- [ ] **Step 1: Failing tests:** `save_and_apply_roundtrip` · `apply_copies_preset_assets` (eigenes Bild landet in `<projekt>/assets/`) · `broken_preset_is_listed_not_applied` · `wrong_version_rejected` · `names_unique_case_insensitive` · `default_preset_used_for_new_doc_without_aspect` · `preset_path_traversal_refused` (Name `../x`) · TS: Anwenden = ein History-Schritt.
- [ ] **Step 2:** Implementieren (Menü im Editor-Kopf: Liste, „Aktuellen Stil speichern…“, „Für neue Projekte verwenden“, Umbenennen, Löschen mit zweistufiger Bestätigung, „Im Finder zeigen“).
- [ ] **Step 3:** Tests grün. Commit `feat(record): style presets`.

---

### Task 11: Projekte wiederöffnen, Doku, Release

**Files:**
- Modify: `core/rust-lib/src/record/project.rs` (`ProjectSummary.updated_at`, Sortierung nach letzter Bearbeitung), `core/rust-lib/src/record/session.rs`, `core/rust-lib/src/record/editor_ipc.rs` (`editor_open` prüft `is_project_dir` für Ordner außerhalb des Projektordners), `commands.rs` + `lib.rs` (`record_rename_project`, `record_open_dialog`), `core/frontend/src/components/RecordPanel.tsx`, `core/frontend/src/components/RecordEditor.tsx` (⌘O-Projektwahl im Editor), Locales, `CHANGELOG.md`, `features.txt`, `CLAUDE.md` (Abschnitt „record — stage 4“), `core/frontend/src/lib/commandDocs.ts`, README via `node scripts/gen-docs.mjs`, Spec (`VP9` streichen, Limiter −2 dBTP), Versionen (0.230.0 → **0.231.0**)
- Test: `project.rs`, `RecordPanel.test.tsx`, `editor_ipc.rs`

**Interfaces:**
- Produces: Liste nach `updated_at` (Fallback `created_at`), Suche (Name/Datum, Unicode-faltend), alle Projekte scrollbar (kein 20er-Deckel), Umbenennen inline (`title`), „Projekt öffnen…“ (Ordnerdialog, auch außerhalb des Projektordners, validiert), Vorschaubild je Projekt aus dem Filmstreifen-Cache des Helfers (fehlt → Platzhalter; kein Helferstart nur für Vorschaubilder), im Editor ⌘O = dieselbe Liste als Dialog (Wechsel speichert vorher, wie Etappe 2).

- [ ] **Step 1: Failing tests:** `list_sorted_by_updated_at` · `rename_changes_title_only` · `open_outside_root_requires_project_dir` · `open_rejects_plain_folder` · Panel: Suche filtert, mehr als 20 Einträge sichtbar per Scrollen, Umbenennen mit Enter/Esc.
- [ ] **Step 2:** Implementieren; Doku-Vertrag (CLAUDE.md „Documentation contract“): `features.txt`-Zeilen (Annotationen, Audiospuren/Voiceover, Clip-Operationen, Tastenkürzel, Presets, Projektliste), CHANGELOG `## [0.231.0] — <Datum>` englisch, CommandDoc `record`, `node scripts/gen-docs.mjs`, Spec-Korrekturen.
- [ ] **Step 3: Ende-zu-Ende über Rust** (keine Tastatureingaben in die App): echtes Projekt (frische Aufnahme, sonst vorhandene — im Bericht sagen), Dokument mit Text (Caveat, Animation `pop`), Pfeil, Mosaik frei, Musik mit Loop, Voiceover-WAV (erzeugt), Preset angewendet; MP4 exportieren, Standbild ansehen, Pegelverlauf messen; Zahlen in den Bericht. Selbst erzeugte Aufnahmen danach löschen.
- [ ] **Step 4:** `bash scripts/check.sh` + `pnpm update-badges` grün (Startpaket ≤ 400 KB — sonst Code verlagern, Grenze nicht anheben). Commit `feat(record): reopen projects, docs (v0.231.0)`. Installation macht der Controller nach der Gesamtprüfung.
