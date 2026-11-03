# `record` Etappe 6 — Untertitel und Transkript-Schnitt — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Der Editor transkribiert jede Aufnahme eines Projekts lokal mit Wortzeiten, blendet daraus Untertitel ein (Stil wie OpenScreen, Vorschau = Export), lässt einzelne Wörter korrigieren und schneidet das Video, wenn man Wörter im Transkript löscht.

**Architecture:** Die Wortzeiten liefert der vorhandene Helfer `ir-transcribe` (whisper-rs, Metal) über einen neuen Befehl `TranscribeWords`: ein zweiter Whisper-Kontext ohne Flash-Attention mit DTW-Preset `LargeV3Turbo`, Wortgrenzen aus `t_dtw`. Das Transkript liegt im Editor-Dokument (v5, damit Undo es mitnimmt), je Aufnahme in Quellzeit. Untertitel werden **nicht gespeichert**, sondern bei jedem Szenenbau in `record/captions.rs` aus den Wörtern und den Clips abgeleitet und als Text-Annotationen (`space:"frame"`, `verticalAlign`) an den Compositor gegeben — kein Vendor-Patch. Der Schnitt ist ein Clip-Schnitt (teilen + Mitte löschen); `splitAt` verteilt Regionen, die über die Schnittstelle reichen, künftig auf **beide** Hälften, damit Zooms und Annotationen einen Schnitt überleben.

**Tech Stack:** Rust (`transcribe-core`/`-helper`/`-protocol`, `core/rust-lib/src/record/`), React 19 + TS, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (Etappe 6, Abschnitte „Editor“ → Transkript-Schnitt, „Untertitel“). Recon mit Belegen: `vendor/openscreen-compositor/RECON-stage6.md` (`C/` = Upstream-Klon im Scratchpad, nur lesen, nichts ausführen).

## Global Constraints

- Nur macOS Apple Silicon (wie Etappen 2–5); Linux/Windows/Intel bauen unverändert grün. Windows-Cross-Check ohne neue Diagnosen.
- Untertitel-Stil, Bereiche und Standards **wörtlich** aus RECON §6 (Code-Werte, nicht Doku): Schrift 48 px bei 1080 (Bedienung 16–140, Lese-Klemme 12–200), `Inter`, fett, `#ffffff`, Platte an `#000000` 0.55 (0.4–0.9; Deckkraft < 0.2 liest sich als aus), unten/mittig, `insetY` 1.5 bei Seitenverhältnis ≥ 1.5 sonst 12.5 (0–50, Schritt 0.5), `insetX` 10 bzw. 5 (0–25, Schritt 0.5, bei mittig ignoriert), Wörter je Zeile 2/7 (1–12, min ≤ max). Konstanten: Bezugshöhe 1080, Zeilenhöhe 1.5 em, 3 Box-Zeilen, Plattenzuschlag 0.2, Querformat ab 1.5, z-Basis 100000, Lauf-Bruch 0.24 s, Dedupe-Lücke 0.55 s / Überlappung 15 ms, Mindest-Zeilenspanne 0.02 s, Schnitt-Atem 0.07 s, Schnitte zusammenlegen unter 2 Bildern (30 fps Rückfall). Vier Looks `classic|bold|minimal|light` wie RECON §6.
- `insetY`/`insetX` werden als `Option` gespeichert; `None` heißt „seitenverhältnisabhängiger Standard“ und darf beim ersten Schreiben **nicht** eingefroren werden.
- Untertitel werden nie als Annotationen ins Dokument geschrieben.
- Wortzeiten: `t_dtw` ist das ENDE eines Tokens — Wortanfang = `t_dtw` des vorigen Text-Tokens (erstes Wort eines Segments: Segmentanfang), Wortende = `t_dtw` des letzten eigenen Tokens. Zeiten in Millisekunden der Datei; das Segment-Offset aus dem eigenen Segmenter wird addiert.
- Quellzeit eines Worts = Dateizeit + Versatz der Tonquelle: `mic.wav` → `+ mic.offset_ms` (bzw. `TakeMeta.mic_offset_ms`); Systemton aus `screen.mp4` → `+ 0`. Quelle je Aufnahme: Mikrofon, wenn vorhanden, sonst Systemton; ohne beides keine Transkription (Meldung).
- Sprachen nur `de|en|auto` (vorhandenes `Lang`). Modell = das in den `meeting`-Einstellungen gewählte; fehlt es, Hinweis mit Download über die vorhandene `meeting_model_download`.
- `EDITOR_VERSION` 4 → **5**; ein v4-Dokument öffnet unverändert (keine Transkripte, Untertitel aus). `PROJECT_SCHEMA` bleibt 1; Preset-Format unverändert (Untertitel gehören nicht in Presets).
- Ohne Transkript und mit Untertiteln aus bleibt die Szene **byte-gleich** (beide Goldens unberührt).
- Kein neues npm-Paket, keine neue Rust-Abhängigkeit (whisper-rs ist schon da). Alle UI-Texte en+de, `.claude/rules/animation.md`, nie `window.confirm`, nie ein Fenster aus einem sync-Befehl.
- Laufwerk knapp: `CARGO_INCREMENTAL=0`, `df -h /System/Volumes/Data` vor schweren Builds (Abbruch unter 1,5 GB); Compositor/Helfer mit `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2`.

## Entscheidungen dieses Plans (Rulings)

- **Schnitt = Clip-Schnitt statt Upstreams `trimRanges`.** Ein Wort-Schnitt teilt den Clip zweimal und löscht die Mitte; nahe einer Clip-Kante (< `MIN_CLIP_MS` 100) wird die Kante verschoben. Das spart ein zweites Schnittmodell, das jede Zeitabbildung, die Zeitleiste und die Szene kennen müssten. Dafür ändert sich `splitAt`: eine Zoom- oder Annotations-Region, die über die Schnittstelle reicht, geht auf **beide** Hälften (Upstream-Invariante „Regionen überleben Schnitte“); das gilt auch für den Schnitt mit S. Kosten falls falsch: manuelle Splits zeigen eine Region nun in zwei Teilen statt gekürzt — sichtbar, per Undo umkehrbar.
- **Wiederherstellen = zwei Nachbar-Clips verschmelzen:** zwei aufeinanderfolgende Clips derselben Aufnahme mit `a.end_ms ≤ b.start_ms` werden zu einem; doppelte Regionen, die genau an der Lücke aneinanderstoßen, werden wieder eins. Der Kopf/das Ende einer Aufnahme (außerhalb aller Clips) wird über die Trim-Griffe zurückgeholt, nicht über das Transkript.
- **Transkript im Dokument** (`EditDoc.transcripts`), nicht als eigene Datei: Undo, Speichern und Projekt-Wechsel funktionieren ohne weitere Arbeit. Größe ~60 Bytes je Wort.
- **Neu-Transkribieren übernimmt Korrekturen** (`carryOverWordEdits`: gleicher `originalText`, überlappende Zeit).
- **Kein Karaoke, keine Wortmarkierung im Bild**, keine Übersetzung, kein SRT-Export, keine Wort-Einfügung (wie Upstream bzw. außerhalb der Spec).
- **Kürzel T** öffnet den Transkript-Tab (frei; C ist bei uns Zuschneiden).
- **DTW-Kontext je Auftrag geladen und danach freigegeben** (zweite Modell-Ladung ~574 MB nur während der Transkription); ein laufendes Meeting/Diktat blockiert den Auftrag mit `transcribe.busy`.

## Review Focus

1. **Wortzeiten verrutschen** — Anfang aus dem eigenen statt dem vorigen Token, oder fehlendes Segment-Offset: Untertitel kommen ~0,2 s zu spät bzw. nach jedem Segment falsch. Test in Task 1 mit synthetischen Tokens.
2. **Ein Wort-Schnitt zerstört Zooms/Annotationen** — Region über die Schnittstelle muss danach in beiden Hälften liegen und das Verschmelzen muss sie wieder zu einer machen. Tests in Task 3.
3. **Untertitel nach Clip-Änderung** — dupliziert, umgeordnet, Tempo, zweite Aufnahme: Zeile erscheint in jedem Clip, der sie abspielt, geklemmt auf das Clip-Fenster, nie außerhalb. Tests in Task 2.
4. **Projekt ohne Mikrofon** — Systemton aus `screen.mp4` wird transkribiert; ganz ohne Ton eine klare Meldung statt Fehler. Test in Task 4.
5. **Hochformat-Ausgabe** — 9:16 bekommt `insetY` 12.5 und Spalte 90 %, auch nachdem der Nutzer andere Felder geändert hat. Test in Task 2.

---

### Task 1: Wortzeiten im Helfer (`TranscribeWords`)

**Files:**
- Create: `transcribe-core/src/words.rs` (pure: Token → Wörter), Test darin
- Modify: `transcribe-core/src/engine.rs` (DTW-Kontext), `transcribe-core/src/file.rs` (Segment-Offset behalten, Wort-Variante), `transcribe-core/src/lib.rs`, `transcribe-protocol/src/lib.rs`, `transcribe-helper/src/serve.rs`, `core/rust-lib/src/transcribe_client.rs` (Dispatch des neuen Events)
- Test: `transcribe-helper/tests/helper_protocol.rs` (ignored Live-Test)

**Interfaces:**
- Produces (protocol): `Command::TranscribeWords { path: PathBuf, lang: Lang, model: String }`; `Event::FileWords { words: Vec<Word>, lang: Option<String> }`; `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct Word { pub text: String, pub start_ms: u64, pub end_ms: u64, pub p: f32 }`. Fortschritt/Fehler/Abbruch wie `TranscribeFile` (`FileProgress`, `FileError`, `CancelFile`).
- Produces (core): `pub fn words_from_tokens(seg_start_ms: u64, seg_end_ms: u64, tokens: &[Tok]) -> Vec<Word>` mit `pub struct Tok { pub text: String, pub t_dtw_cs: i64, pub p: f32, pub special: bool }`; `pub fn filter_words(words: Vec<Word>) -> Vec<Word>` (dieselben Schleifen-/Wiederholungsregeln wie `drop_loops` auf Wortebene); `Engine::load_dtw(path: &Path) -> Result<Engine, String>`.

- [ ] **Step 1: Tests für `words_from_tokens`**: `joins_bpe_tokens_at_a_leading_space` (`[" Hal","lo"," Welt"]` → 2 Wörter), `start_is_the_previous_tokens_end` (Token-Enden 50/80/120 cs, Segment ab 1000 ms → Wort 2 beginnt bei 1000+800 ms), `first_word_starts_at_segment_start`, `special_tokens_are_skipped`, `missing_dtw_falls_back_to_even_spread` (`t_dtw_cs == -1` überall → Wörter gleichmäßig über das Segment), `times_are_monotonic_and_clamped_to_the_segment`, `punctuation_only_token_attaches_to_previous_word`.
- [ ] **Step 2: Rot sehen**: `cargo test -p transcribe-core --lib words`
- [ ] **Step 3: Implementieren**; `Engine::load_dtw`: `use_gpu(true).flash_attn(false)` + `dtw_parameters(DtwParameters{ mode: DtwMode::ModelPreset{ model_preset: DtwModelPreset::LargeV3Turbo }, ..Default })`; Decode greedy mit `set_token_timestamps(true)`, sonst wie die finale Decode (Sprache, `no_context`, Stille −50 dBFS übersprungen). `file.rs`: Variante `transcribe_words_stream` nutzt `Seg.start` (Samples → ms bei 16 kHz) als Offset.
- [ ] **Step 4: Protokoll-Rundtest** in `transcribe-protocol` (`TranscribeWords` und `FileWords` serialisieren mit `cmd`/`event`-Tag `transcribe_words`/`file_words`), Helfer bedient den Befehl wie `TranscribeFile` (gleiche Busy-Regel `ERR_BUSY`), `transcribe_client` leitet `FileWords` an den Auftrag weiter.
- [ ] **Step 5: Grün**: `cargo test -p transcribe-core -p transcribe-protocol`; `cargo clippy -p transcribe-core -p transcribe-helper -- -D warnings`
- [ ] **Step 6: Live-Test (ignored)** `file_words` in `helper_protocol.rs`: `say -o` eine deutsche WAV mit bekanntem Satz, erwartet die Wörter in Reihenfolge, monotone Zeiten, letztes Wortende ≤ Dateilänge + 200 ms. Ausführen: `cargo test --release -p transcribe-helper --test helper_protocol file_words -- --ignored --nocapture`.
- [ ] **Step 7: Commit** `feat(transcribe): word timestamps for whole files`

### Task 2: Untertitel ableiten, Dokument v5, Szene

**Files:**
- Create: `core/rust-lib/src/record/captions.rs` (pure), Tests darin
- Modify: `core/rust-lib/src/record/editor.rs` (v5, Typen, migrate, normalize), `core/rust-lib/src/record/scene_json.rs` (Untertitel-Annotationen, `verticalAlign`), `core/rust-lib/src/record/mod.rs`, `core/frontend/src/lib/ipc.ts` (Typen), `core/frontend/src/lib/editor.ts` (TS-Spiegel der Konstanten)

**Interfaces:**
- Produces (editor.rs): `EDITOR_VERSION = 5`; `EditDoc.transcripts: Vec<Transcript>` (serde default, lenient), `EditDoc.captions: CaptionSettings` (serde default). `Transcript { source: Option<String> /* None = eigene Aufnahme, sonst Take-Id */, lang: String, words: Vec<TranscriptWord> }`; `TranscriptWord { id: String, start_ms: u64, end_ms: u64, text: String, original_text: Option<String> }` (Quellzeit der Aufnahme); `CaptionSettings { enabled: bool, font_size: f64, font_family: String, bold: bool, color: String, plate: bool, plate_color: String, plate_opacity: f64, anchor_v: CaptionAnchorV /* top|bottom */, anchor_h: CaptionAnchorH /* left|center|right */, inset_y: Option<f64>, inset_x: Option<f64>, min_words: u8, max_words: u8, lang: Lang }`. Serde camelCase wie der Rest.
- Produces (captions.rs): `pub struct CaptionLine { pub start_ms: u64, pub end_ms: u64, pub text: String }`; `pub fn caption_lines(words: &[TranscriptWord], min: u8, max: u8) -> Vec<CaptionLine>` (Gruppierung + Dedupe, leere Wörter ausgelassen); `pub fn caption_box(settings: &CaptionSettings, out: (u32, u32)) -> (f64, f64, f64, f64)` (Prozent x, y, w, h); `pub fn caption_entries(doc: &EditDoc, out: (u32, u32), kept: &dyn Fn(usize) -> Option<usize>) -> Vec<serde_json::Value>`; `pub const CAPTION_LOOKS: [(&str, CaptionLook); 4]`.

- [ ] **Step 1: Tests in `captions.rs`**: `groups_runs_at_a_quarter_second_gap` (Lücke 0.24 s bricht, 0.23 nicht), `lines_respect_min_and_max_words` (11 Wörter, 2/7 → kein Zeilenende mit 1 Wort), `line_span_is_at_least_20ms`, `same_text_close_together_is_merged`, `empty_word_text_is_hidden`, `box_landscape_bottom_centre` (16:9, Standards → x 16, w 68, y = 100 − 1.5 − h), `box_portrait_uses_12_5_and_90_column` (9:16, auch wenn `font_size` geändert wurde), `entries_repeat_in_duplicated_clips`, `entries_are_clamped_to_the_clip_window_and_split_at_clip_edges`, `entries_follow_each_takes_own_transcript`, `entries_sort_after_every_annotation` (echte Annotation mit z 500000 → Untertitel danach), `entries_carry_vertical_align_and_rgba_plate`, `disabled_emits_nothing`.
- [ ] **Step 2: Tests in `editor.rs`**: `v4_doc_opens_with_captions_off_and_no_transcripts`, `normalize_clamps_caption_settings` (Schrift 300 → 200, Deckkraft 0.1 bleibt 0.1 und gilt als aus, min > max → getauscht), `transcript_words_are_sorted_and_clamped_to_the_take`, `inset_none_survives_a_save_roundtrip`.
- [ ] **Step 3: Rot sehen**, dann implementieren. `scene_json` hängt die Einträge an die Annotationsliste an (nach `annotations_json`, eigene ids `caption-<clip>-<n>`), `fontSizeRel = font_size/1080`, `backgroundColor = rgba(r,g,b,opacity)` bzw. transparent ohne Platte, `textAlign = anchor_h`, `verticalAlign = anchor_v`, `textAnimation`/`animation = "none"`.
- [ ] **Step 4: Goldens prüfen**: bestehende Szenen-Goldens unverändert (`cargo test -p inspector-rust-core --lib scene_json`); neuer Golden-Test `scene_with_captions` mit kleinem Transkript.
- [ ] **Step 5: TS-Spiegel**: `CAPTION_DEFAULTS`, Bereiche und Looks in `lib/editor.ts`; Test in `editor.test.ts` liest `captions.rs` und vergleicht die Zahlen (Muster der bestehenden Cross-Language-Pins).
- [ ] **Step 6: Grün + Clippy**, Commit `feat(record): captions derived from the transcript`

### Task 3: Schnitt über Wörter, Regionen überleben Schnitte

**Files:**
- Create: `core/frontend/src/lib/transcript.ts` (pure), `core/frontend/src/lib/transcript.test.ts`
- Modify: `core/frontend/src/lib/editor.ts` (`splitAt`, neue Ops), `core/frontend/src/lib/editor.test.ts`

**Interfaces:**
- Produces: `cutSourceRange(doc: RecordEditDoc, clip: number, startMs: number, endMs: number): RecordEditDoc` (teilt zweimal + löscht Mitte; Kante näher als `MIN_CLIP_MS` → `setTrim`; ganzer Clip → `deleteClip`, außer es ist der letzte); `canRestoreGap(doc, clip: number): boolean`, `restoreGap(doc, clip: number): RecordEditDoc` (verschmilzt `clip` und `clip+1`); `breathe(startMs: number, endMs: number, words: ClipWord[]): [number, number]` (70 ms Atem, höchstens bis zur Lückenmitte, nur an behaltenen Wörtern); `clipWords(doc, transcripts, clip: number): ClipWord[]` mit `ClipWord { id: string /* clipIndex:wordId */, wordId: string, startMs: number, endMs: number, text: string, edited: boolean }`; `cutWords(doc, clip, wordIds: string[]): RecordEditDoc`; `gaps(doc, transcripts, clip): { afterClip: number, words: ClipWord[] }[]` (weggeschnittene Wörter zwischen `clip` und `clip+1`); `setWordText(doc, source: string | null, wordId: string, text: string): RecordEditDoc` (erstes Ändern speichert `originalText`, das Original zurücktippen löscht es, Text wird getrimmt); `carryOverEdits(old: Transcript, next: Transcript): Transcript`.

- [ ] **Step 1: Tests `editor.test.ts`**: `split_puts_a_spanning_zoom_into_both_halves`, `split_puts_a_spanning_annotation_into_both_halves` (beide mit neuen ids, Zeiten an der Schnittstelle geteilt), `cut_source_range_removes_the_middle`, `cut_near_an_edge_moves_the_edge`, `cut_of_the_whole_clip_deletes_it_but_never_the_last`, `restore_gap_merges_neighbours_and_rejoins_split_regions`, `restore_gap_refuses_different_takes_or_reordered_clips`.
- [ ] **Step 2: Tests `transcript.test.ts`**: `breathe_keeps_70ms_next_to_kept_words`, `breathe_stops_at_the_gap_midpoint`, `breathe_leaves_edges_inside_words`, `cut_words_selection_spans_first_to_last`, `cut_words_merges_cuts_under_two_frames`, `clip_words_only_inside_the_clip_window`, `gaps_list_the_cut_words_between_neighbours`, `set_word_text_keeps_and_clears_original`, `carry_over_keeps_user_corrections_on_overlap`.
- [ ] **Step 3: Rot sehen**: `pnpm --filter inspector-rust-frontend exec vitest run src/lib/editor.test.ts src/lib/transcript.test.ts`
- [ ] **Step 4: Implementieren**; die Region-Teilung in `splitAt` gibt der zweiten Hälfte `crypto.randomUUID()`; `restoreGap` erkennt zusammengehörige Hälften an gleichem Inhalt außer id/clip/Zeiten und `a.endMs === cut && b.startMs === cut`.
- [ ] **Step 5: Grün**, Commit `feat(record): cut the video by deleting words`

### Task 4: Transkription aus dem Editor starten

**Files:**
- Modify: `core/rust-lib/src/record/editor_ipc.rs` (IPC + Ablauf), `core/rust-lib/src/lib.rs` (Registrierung), `core/frontend/src/lib/ipc.ts`
- Test: Tests in `editor_ipc.rs`

**Interfaces:**
- Consumes: Task 1 `Command::TranscribeWords`/`Event::FileWords`, `transcribe_client::request`; Task 2 Typen.
- Produces: `#[tauri::command] pub async fn editor_transcribe(app, session_id: u64, lang: Lang) -> Result<EditorView, String>` (alle Aufnahmen nacheinander, Fortschritt als Event `record-editor-transcribe {done, total, source}`; Ergebnis als **ein** Undo-Schritt über die Update-Kette wie `editor_auto_zoom`); `editor_transcribe_cancel()`; pure `pub fn audio_source(project_dir: &Path, meta: &RecordingMeta) -> Option<(PathBuf, u64 /* offset_ms */)>`; pure `pub fn to_transcript(source: Option<String>, words: Vec<Word>, offset_ms: u64, lang: Option<String>, old: Option<&Transcript>) -> Transcript` (Offset addiert, ids `w<n>`, Korrekturen übernommen).

- [ ] **Step 1: Tests**: `audio_source_prefers_the_mic_with_its_offset`, `audio_source_falls_back_to_system_audio`, `audio_source_none_without_any_audio`, `to_transcript_adds_the_offset`, `to_transcript_carries_corrections_over`, `transcribe_refuses_while_exporting_or_another_session_is_open` (Sentinels `record.editor.transcribe_busy`, `record.editor.no_audio`).
- [ ] **Step 2: Rot**, implementieren: Modell aus `meeting::config().model`, fehlt es → `model.missing`; Busy des Helfers → `transcribe.busy`; Abbruch → `CancelFile`, schon fertige Aufnahmen bleiben.
- [ ] **Step 3: Grün + Clippy** (`IR_SKIP_BADGES=1`), Commit `feat(record): transcribe a project from the editor`

### Task 5: Oberfläche — Untertitel-Tab, Transkript-Tab, Kürzel T

**Files:**
- Create: `core/frontend/src/components/editor/CaptionsTab.tsx`, `TranscriptTab.tsx` + Tests
- Modify: `core/frontend/src/components/editor/Inspector.tsx` (Tabs `captions`, `transcript`), `core/frontend/src/lib/editor-shortcuts.ts` (`transcript: { key: "t" }`), `core/frontend/src/components/RecordEditor.tsx`, `core/frontend/src/locales/en.json` + `de.json`, `src/mockup/backend.ts`

**Interfaces:**
- Consumes: Task 2 Typen/Konstanten, Task 3 Ops, Task 4 IPC.

- [ ] **Step 1: Tests** `CaptionsTab.test.tsx`: Schalter an/aus schreibt `captions.enabled`; Look „bold“ setzt Oswald 60 `#fde047` Platte 0.85; Plattenwahl hell kippt die Textfarbe auf `#111111`; Schriftgröße außerhalb 16–140 wird geklemmt; ohne Transkript zeigt der Tab „Transkribieren“ mit Sprach-Chips DE/EN/Auto; fehlendes Modell zeigt den Download-Knopf. `TranscriptTab.test.tsx`: ein Block je Clip; Klick auf ein Wort springt zur Wortzeit (Programmzeit aus Rusts `editor_to_programme(clip, sourceMs)`, nie in TS gerechnet); ⌫ auf markierten Wörtern ruft `cutWords` als ein Undo-Schritt; durchgestrichene Lücke wiederherstellen ruft `restoreGap`; Doppelklick bearbeitet, Enter speichert, Esc verwirft, leer blendet das Wort aus; das gerade gesprochene Wort ist hervorgehoben.
- [ ] **Step 2: Rot**, implementieren; Bearbeitungen laufen über `push` der Editor-Historie; die Tabs sind wie die anderen lazy-frei in `Inspector`; Fortschritt aus `record-editor-transcribe`.
- [ ] **Step 3: Kürzel**: `transcript` in Bindings + `ShortcutsDialog`-Liste; Test, dass T den Tab öffnet und im Textfeld nicht feuert.
- [ ] **Step 4: Mockup** mit erfundenem Transkript; Sichtprüfung in `mockup.html` (Hell/Dunkel, Deutsch, 700 px).
- [ ] **Step 5: Grün** (`IR_SKIP_BADGES=1 pnpm test`, `pnpm typecheck`, `pnpm lint`), Commit `feat(record): captions and transcript tabs`

### Task 6: Doku, Version, Installation, Ende-zu-Ende

**Files:** `CHANGELOG.md`, `features.txt`, `CLAUDE.md` (Abschnitt „record — stage 6“), `docs/` falls ein record-Dokument existiert, Versionsdateien (Cargo.toml, alle package.json, drei tauri.conf.json), `core/frontend/src/lib/commandDocs.ts` (record-Doku um Untertitel ergänzen), README-Badges.

- [ ] **Step 1:** Version 0.236.0 → 0.237.0, CHANGELOG-Eintrag (englisch), `features.txt`-Zeilen, CLAUDE.md-Abschnitt mit den Rulings und Fallstricken dieses Plans.
- [ ] **Step 2:** `bash scripts/check.sh`, `pnpm test` (Badges), `cargo test --workspace` grün.
- [ ] **Step 3:** `scripts/install-macos.sh`, Bundle enthält `ir-transcribe`; Ende-zu-Ende in der installierten App: Projekt mit Mikrofon öffnen, transkribieren, Untertitel an, Wort löschen → Video geschnitten, Zoom über der Stelle bleibt, Export zeigt die Untertitel.
- [ ] **Step 4:** Commit + Push.
