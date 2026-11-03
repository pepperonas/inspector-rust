# Live-Transkription (MIC + SYS) mit Ticker — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mikrofon und Systemton gleichzeitig lokal transkribieren (whisper.cpp/Metal), absturzsicher in Textdateien schreiben, als Laufband zeigen — und die alte Python-Transkription durch dieselbe Engine ersetzen.

**Architecture:** Drei neue Crates: `transcribe-protocol` (plattformfrei: Wire-Typen + Session-Dateiformat + Recovery, von der App benutzt), `transcribe-core` (Audio, VAD, Segmentierung, LocalAgreement, Dedupe, Writer, Modelle, whisper-rs — keine Tauri-Abhängigkeit) und `transcribe-helper` (Binary `ir-transcribe`, JSON-Zeilen über stdin/stdout). Die App startet den Helper (Muster `ir-ocr`), übersetzt seine Events in Tauri-Events, Tray, Hotkeys, Ticker-Fenster, Panel und Settings. whisper.cpp wird nie ins App-Binary gelinkt.

**Tech Stack:** Rust 2021, whisper-rs 0.16 (`metal`, whisper.cpp 1.8.3 inkl. Silero-VAD in ggml), rubato 0.16, hound 3.5, cpal 0.15, objc2 0.6 (Process Tap, ScreenCaptureKit, AVAssetReader), serde_json, ureq (Download), sha2; Tauri 2; React 19 + TS.

**Spec:** `docs/superpowers/specs/2026-11-03-live-transcription-design.md`

## Global Constraints

- Arbeit im Worktree `/Users/martin/claude/inspector-rust-live-transcribe`, Branch `feat/live-transcribe`. Nie im Haupt-Checkout bauen oder committen (dort arbeitet die Aufnahme-Session). Eigenes `target/`, kein geteiltes `CARGO_TARGET_DIR`.
- Transkription nur auf Apple-Silicon-Macs: `transcribe-core`/`transcribe-helper` bauen nur für `cfg(all(target_os = "macos", target_arch = "aarch64"))`-Features; die App hat für andere Plattformen einen Stub, der `transcribe.unsupported` zurückgibt; alle `transcribe`/`meeting`-CommandSpecs bekommen `platform: ["mac"]`.
- Kein Python mehr am Ende (Task 16): `assets/transcribe/*.py`, `scripts/setup-transcribe.py`, venv-Check entfernen.
- Audio intern: 16 000 Hz mono f32. Segment-Vorlauf/Nachlauf je 300 ms, Max-Segment 15 s, erzwungener Schnitt an der leisesten Stelle der letzten 3 s.
- Ticker-Verzögerung Ziel ≤ 3 s; Hypothesen-Takt 1 s; LocalAgreement-2 (zwei aufeinanderfolgende Hypothesen).
- Dedupe-Standardschwelle 0.75 (normalisierte Levenshtein-Ähnlichkeit), Überlapp-Toleranz 1 s, MIC-Final-Wartezeit max. 3 s.
- Sprache: `auto | de | en`; Auto vergleicht nur de- und en-Wahrscheinlichkeit.
- Konfidenzregeln aus dem alten Worker: Segment verwerfen wenn mittlere Token-Logprob < −1.0 oder Kompressionsrate > 2.4; ≥ 3 identische Folgezeilen = Schleife, auf 2 kürzen.
- Modelle unter `~/Library/Application Support/InspectorRust/models/`, Standard `ggml-large-v3-turbo-q5_0.bin`. Katalog (Datei, Bytes, SHA-256) exakt:
  - `ggml-large-v3-turbo-q5_0.bin` 574041195 `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2`
  - `ggml-large-v3-turbo-q8_0.bin` 874188075 `317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1`
  - `ggml-medium-q5_0.bin` 539212467 `19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f`
  - `ggml-small-q5_1.bin` 190085487 `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb`
  - URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/<datei>`
- VAD-Modell eingebettet: `ggml-silero-v6.2.0.bin`, 885098 Bytes, SHA-256 `2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987`, von `https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin` (MIT, Lizenzhinweis in `THIRD-PARTY`).
- Session-Ordner Standard `~/Documents/Transkripte/YYYY-MM-DD_HHMMSS/`, Fallback `~/Library/Application Support/InspectorRust/Transkripte/`. Dateien `transkript.txt` (`[HH:MM:SS] MIC: text`), `transkript.jsonl`, `session.json` (Status `running|completed|aborted`), optional `audio/<seq>-<mic|sys>.wav`. Append + `flush` + `sync_data` pro Segment; nur `session.json` atomar (tmp+rename).
- Rohaudio Standard an, Aufbewahrung Standard 7 Tage.
- Haus-Regeln (CLAUDE.md): jeder neue sichtbare Text in `locales/en.json` **und** `de.json`; Doku-Vertrag (features.txt, CommandDoc + gen-docs, CHANGELOG englisch, CLAUDE.md); blockierende Tauri-Commands `async` + `spawn_blocking`; Fenster nie aus sync Commands bauen; Overlays `orderFrontRegardless` + `setCanHide:NO`; `confirmDialog` statt `window.confirm`; App-Chunk-Budget 400 KB (neue Panels lazy); jede neue Zusicherung einmal mutationsgeprüft.
- Settings-Footer im Settings-Tab: `© {aktuelles Jahr} Martin Pfeffer | celox.io` (Jahr aus `new Date().getFullYear()`), Muster wie `ClaudeLimitsPanel.tsx:539`.
- Commits enden mit den Attributions-Zeilen der Session.

## Review Focus

1. **Mikrofon oder Systemton während der Session weg** (USB-Mic abgezogen, Kopfhörer gewechselt, Ausgabegerät wechselt auf boom Audio): die andere Quelle läuft weiter, die betroffene versucht neu zu verbinden, Tray/Panel zeigen es — Test in Task 9 (Pipeline mit einer Quelle, die einen Fehler liefert).
2. **Lange Stille / reine Musik über Stunden:** keine wachsenden Puffer, keine erfundenen Texte („Untertitel von …"), kein Speicherwachstum — Test in Task 4 (Segmenter hält bei 2 h Stille-Feed `pending` ≤ 3 s) und Task 6 (Rauschtext-Filter).
3. **Sprachwechsel mitten im Gespräch (de ↔ en):** pro Final neu entschieden; Hypothese nutzt die Sprache des letzten Finals — Test in Task 7 (`pick_de_en`) und Task 8 (Engine-Aufruf mit erzwungener Sprache).
4. **Zielordner nicht beschreibbar / Platte voll beim Start oder mittendrin:** Fallback-Ordner, Warnung, nie still weiter — Test in Task 3 (Writer mit read-only Ordner).
5. **App wird mit `kill -9` beendet, Helper lebt weiter:** Helper sieht EOF, schließt ab (`completed`), beendet sich; beim nächsten App-Start keine Doppel-Session — Test in Task 11 (Helper-Prozesstest mit geschlossenem stdin).

---

## Phase 1 — Pure Kern (keine Audio-/GPU-Abhängigkeit)

### Task 1: Crates anlegen, `transcribe-protocol` mit Wire-Typen

**Files:**
- Create: `transcribe-protocol/Cargo.toml`, `transcribe-protocol/src/lib.rs`
- Create: `transcribe-core/Cargo.toml`, `transcribe-core/src/lib.rs`
- Create: `transcribe-helper/Cargo.toml`, `transcribe-helper/src/main.rs` (vorerst `fn main(){}`)
- Modify: `Cargo.toml` (members += die drei; workspace deps `whisper-rs = { version = "0.16", features = ["metal"] }`, `rubato = "0.16"`, `hound = "3.5"`)

**Interfaces:**
- Produces (`transcribe_protocol`):
  - `enum Source { Mic, Sys }` (serde `"mic"|"sys"`), `fn label(self) -> &'static str` → `"MIC"`/`"SYS"`.
  - `enum Lang { Auto, De, En }` (serde lowercase).
  - `struct SessionConfig { sources: Vec<Source>, mic_device: Option<String>, model: String, lang: Lang, root: PathBuf, keep_audio: bool, dedupe: DedupeMode, dedupe_threshold: f32, mode: Mode }`; `enum DedupeMode { Auto, Always, Never }`; `enum Mode { Meeting, Dictation, Song, Room }`.
  - `enum Command` (tag `"cmd"`, snake_case): `Start(SessionConfig)`, `Stop`, `SetSource{source, on: bool}`, `Window{source, secs: u32}`, `DownloadModel{model: String}`, `CancelDownload`, `TranscribeFile{path: PathBuf, lang: Lang, model: String}`, `CancelFile`, `ListDevices`, `Shutdown`.
  - `enum Event` (tag `"event"`, snake_case): `Ready{version: String}`, `Started{session_dir: PathBuf, sources: Vec<Source>}`, `Hypothesis{source, seq: u64, stable: String, pending: String}`, `Final{source, seq: u64, start_ms: u64, end_ms: u64, lang: String, text: String, confidence: f32}`, `Dropped{source, seq, reason: String}`, `Level{source, rms: f32}`, `SourceState{source, state: SourceStatus, detail: Option<String>}`, `Warning{code: String, detail: String}`, `Stopped{session_dir: Option<PathBuf>, error: Option<String>}`, `Window{source, pcm_b64: String}`, `ModelProgress{model, done: u64, total: u64}`, `ModelReady{model}`, `ModelError{model, error: String}`, `FileProgress{done_ms: u64, total_ms: u64}`, `FileDone{txt: PathBuf}`, `FileError{error: String}`, `Devices{mic: Vec<String>}`, `Error{error: String}`.
  - `enum SourceStatus { Starting, Running, Lost, Off }`.
  - `fn encode<T: Serialize>(v: &T) -> String` (eine Zeile, `\n`-terminiert), `fn decode_command(line: &str) -> Result<Command, String>`, `fn decode_event(line: &str) -> Result<Event, String>`.

- [ ] **Step 1:** Test `roundtrip_every_command_and_event` — jede Variante `encode` → `decode_*` gleich; `encode` enthält genau ein `\n` am Ende auch wenn `text` Zeilenumbrüche enthält.
- [ ] **Step 2:** Test `unknown_command_is_an_error_not_a_panic` — `decode_command("{\"cmd\":\"nope\"}")` → `Err`.
- [ ] **Step 3:** Implementieren (serde `#[serde(tag = ...)]`, `PartialEq` derive).
- [ ] **Step 4:** `cargo test -p transcribe-protocol` → PASS; `cargo clippy -p transcribe-protocol -- -D warnings` sauber.
- [ ] **Step 5:** Commit `feat(transcribe): protocol crate and workspace scaffold`.

### Task 2: Text-Werkzeuge — Normalisierung, Ähnlichkeit, LocalAgreement, Filter

**Files:**
- Create: `transcribe-core/src/text.rs`

**Interfaces:**
- Produces:
  - `fn normalize(s: &str) -> String` — Kleinschreibung, Satzzeichen weg, Mehrfach-Leerzeichen zu einem, Umlaute bleiben.
  - `fn similarity(a: &str, b: &str) -> f32` — `1 − levenshtein(norm a, norm b) / max(len)` auf **Zeichen** (chars), leer/leer = 1.0, leer/nicht-leer = 0.0.
  - `struct Agreement { committed: Vec<String>, last: Vec<String> }` mit `fn update(&mut self, hypothesis: &str) -> (String /*stable*/, String /*pending*/)` — stabil = bisher committete Wörter + längster gemeinsamer Wortpräfix von `last` und neuer Hypothese (Wörter verglichen via `normalize`); committete Wörter werden nie zurückgenommen; `fn reset(&mut self)`.
  - `fn is_noise_text(s: &str) -> bool` — Port von `live_transcribe::is_noise_text` (Code übernehmen) plus bekannte Whisper-Halluzinationen auf Stille: „Untertitel im Auftrag des ZDF“, „Untertitelung des ZDF“, „Vielen Dank fürs Zuschauen“, „Thanks for watching“, „Subtitles by“ (normalisiert, Teilstring).
  - `fn drop_loops(lines: Vec<String>, max_repeat: usize) -> Vec<String>` (Port von `live.py::drop_loops`, `MAX_REPEAT = 2`).
  - `fn compression_ratio(s: &str) -> f32` — `bytes / zlib(bytes)` mit `flate2`.

- [ ] **Step 1:** Tests:
  - `similarity("Hallo, Welt!", "hallo welt") == 1.0`; `similarity("abc","") == 0.0`; `similarity("Das ist ein Test", "Das ist ein Fest") > 0.9`.
  - `agreement_commits_only_what_two_hypotheses_share`: update("ich gehe heute") → stable `""`, pending `"ich gehe heute"`; update("ich gehe heute nach") → stable `"ich gehe heute"`, pending `"nach"`; update("ich ging heute nach Hause") → stable bleibt `"ich gehe heute"` (nie zurücknehmen), pending `"nach Hause"`.
  - `agreement_is_case_and_punctuation_insensitive`: "Hallo, wie" / "hallo wie geht's" → stable `"Hallo, wie"` (Originalschreibung der ersten).
  - `noise_texts_are_recognised`: "Untertitel im Auftrag des ZDF, 2021" → true; "Wir sehen uns morgen" → false.
  - `drop_loops(vec!["a","a","a","b"], 2) == ["b"]`-Semantik wie Python (Läufe > max_repeat fallen ganz weg — Python-Code übernehmen, Test an dessen Verhalten).
  - `compression_ratio("ja ".repeat(200)) > 2.4`; normaler Satz < 2.0.
- [ ] **Step 2:** `cargo test -p transcribe-core text::` → FAIL.
- [ ] **Step 3:** Implementieren.
- [ ] **Step 4:** PASS; Mutationsprobe: in `Agreement::update` das „nie zurücknehmen“ entfernen → dritter Agreement-Test muss rot werden.
- [ ] **Step 5:** Commit `feat(transcribe-core): text normalisation, similarity, local agreement`.

### Task 3: Writer, Session-Dateien, Recovery, Fallback

**Files:**
- Create: `transcribe-protocol/src/session.rs` (Format + Recovery + Liste — die App braucht das ohne Helper)
- Create: `transcribe-core/src/writer.rs`

**Interfaces:**
- Produces (`transcribe_protocol::session`):
  - `struct SessionMeta { version: u32 /*=1*/, started_ms: u64, ended_ms: Option<u64>, status: SessionStatus, model: String, lang: Lang, sources: Vec<Source>, mode: Mode, fallback_from: Option<PathBuf>, note: Option<String> }`; `enum SessionStatus { Running, Completed, Aborted }`.
  - `struct Line { start_ms: u64, end_ms: u64, source: Source, lang: String, confidence: f32, text: String }` (jsonl-Zeile).
  - `fn session_dir_name(started_local: chrono::NaiveDateTime) -> String` → `YYYY-MM-DD_HHMMSS`; bei Kollision Suffix `-2`, `-3` (`fn free_session_dir(root, name) -> PathBuf`).
  - `fn txt_line(l: &Line, local_offset_s: i32) -> String` → `"[19:45:12] MIC: text\n"` (Zeit = Start, lokale Uhrzeit; Zeilenumbrüche im Text durch Leerzeichen ersetzt).
  - `fn write_meta_atomic(dir: &Path, meta: &SessionMeta) -> io::Result<()>` (tmp `session.json.tmp` + fsync + rename).
  - `fn recover(root: &Path) -> Vec<Recovered>` — jeder Unterordner mit `session.json` Status `Running` → `Aborted`, `ended_ms` = `end_ms` der letzten **vollständigen** jsonl-Zeile (eine abgeschnittene letzte Zeile wird beim Lesen ignoriert und per Truncate auf die letzte `\n`-Grenze entfernt, in txt ebenso), Meta neu schreiben; `struct Recovered { dir: PathBuf, lines: usize, ended_ms: Option<u64> }`.
  - `fn list_sessions(roots: &[PathBuf]) -> Vec<SessionSummary>` (neueste zuerst; `SessionSummary { dir, meta, lines }`).
- Produces (`transcribe_core::writer`):
  - `struct Writer` mit `fn open(root: &Path, fallback_root: &Path, meta: SessionMeta, keep_audio: bool) -> Result<Writer, WriteError>`, `fn append(&mut self, line: &Line, audio: Option<&[f32]>) -> Result<(), WriteError>`, `fn finish(self, status: SessionStatus) -> Result<PathBuf, WriteError>`, `fn dir(&self) -> &Path`, `fn used_fallback(&self) -> bool`.
  - Bei `append`-Fehler im Hauptordner: einmal auf neuen Session-Ordner unter `fallback_root` umziehen (Meta mit `fallback_from`), Zeile dort schreiben, `Err(WriteError::FellBack(path))`-artige Meldung **zusätzlich** über `fn take_warnings(&mut self) -> Vec<String>`; scheitert auch Fallback → `Err(WriteError::Fatal(..))`.
  - `fn spawn_writer(w: Writer) -> (Sender<WriterMsg>, JoinHandle<..>)` mit `enum WriterMsg { Line(Line, Option<Vec<f32>>), Finish(SessionStatus) }` und Rückkanal `Receiver<WriterReport>` (`Written{seq}`, `Warning(String)`, `Fatal(String)`, `Finished(PathBuf)`).
  - WAV: `audio/<seq:06>-<mic|sys>.wav`, 16 kHz mono i16, via `hound`, je Segment komplett geschrieben.
  - `fn prune_audio(root: &Path, keep_days: u32, now: SystemTime)` — löscht nur `*/audio/*.wav` älter als N Tage, nie Transkripte.

- [ ] **Step 1:** Tests (tempfile):
  - `append_writes_txt_and_jsonl_and_syncs` — zwei Zeilen → txt hat 2 Zeilen im Format, jsonl 2 parsebare Zeilen.
  - `crash_mid_line_loses_only_that_line` — nach 3 `append` die jsonl + txt per Hand um eine halbe Zeile ergänzen (simulierter Absturz beim Schreiben), Meta bleibt `running` → `recover` liefert `lines == 3`, Status `Aborted`, `ended_ms` = Ende Zeile 3, halbe Zeile entfernt.
  - `kill_after_append_keeps_everything` — Writer ohne `finish` droppen (`std::mem::forget`) → `recover` findet alle Zeilen.
  - `completed_sessions_are_left_alone` — Status `Completed` bleibt unverändert.
  - `unwritable_root_falls_back` — Root mit Rechten 0o500 → `open` landet im Fallback, `used_fallback() == true`, Meta `fallback_from` gesetzt.
  - `failure_midway_moves_to_fallback_and_warns` — nach erstem `append` den Hauptordner auf 0o500 setzen → nächstes `append` schreibt in Fallback, `take_warnings()` nicht leer.
  - `both_roots_broken_is_fatal` → `Err(Fatal)`.
  - `wav_per_segment_is_complete` — hound liest Datei zurück, Länge = Eingabe.
  - `prune_keeps_transcripts` — alte wav weg, txt bleibt.
  - `session_dir_names_never_collide`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implementieren. Dateien mit `OpenOptions::new().append(true).create(true)`, nach jedem Schreiben `flush()` + `sync_data()`. Für den „Hauptordner kaputt“-Test unix-Rechte nutzen (`#[cfg(unix)]`).
- [ ] **Step 4:** PASS. Mutationsproben: `sync_data` entfernen kann kein Test sehen (nur dokumentieren); Truncate-auf-`\n` entfernen → `crash_mid_line…` rot; Fallback-Zweig entfernen → zwei Tests rot.
- [ ] **Step 5:** Commit `feat(transcribe): crash-safe session writer and recovery`.

### Task 4: Segmenter auf VAD-Wahrscheinlichkeiten

**Files:**
- Create: `transcribe-core/src/segment.rs`

**Interfaces:**
- Konstanten: `RATE = 16_000`, `VAD_FRAME = 512` (32 ms, Silero-Fenster), `PAD = RATE * 3 / 10`, `MAX_SEG = RATE * 15`, `CUT_SEARCH = RATE * 3`, `SPEECH_ON = 0.5`, `SPEECH_OFF = 0.35`, `MIN_SILENCE = RATE * 6 / 10` (Pause, die ein Segment beendet), `MIN_SPEECH = RATE / 4`, `KEEP_IDLE = PAD` (Puffer ohne Sprache wird auf PAD gekürzt).
- Produces:
  - `struct Seg { start: u64, end: u64 }` (absolute Samples), `struct OpenSeg { start: u64, end: u64 }`.
  - `struct Segmenter` mit `fn push(&mut self, samples: &[f32], probs: &[f32]) -> Vec<(Seg, Vec<f32>)>` (`probs.len() * VAD_FRAME == samples.len()`; liefert geschlossene Segmente samt Samples inkl. Vor-/Nachlauf), `fn open(&self) -> Option<(OpenSeg, &[f32])>` (laufendes Segment für Hypothesen), `fn flush(&mut self) -> Option<(Seg, Vec<f32>)>`, `fn buffered(&self) -> usize`.
  - Hysterese: Sprache beginnt bei `p ≥ SPEECH_ON`, endet nach `MIN_SILENCE` Samples mit `p < SPEECH_OFF`; Segmente mit < `MIN_SPEECH` Sprachanteil werden nicht geliefert. Bei Erreichen von `MAX_SEG`: Schnitt an dem Frame mit kleinster RMS in den letzten `CUT_SEARCH` Samples; Rest beginnt das nächste Segment ohne Lücke.

- [ ] **Step 1:** Tests (synthetische probs + Samples):
  - `speech_between_silence_is_one_segment_with_padding` — 1 s Stille, 2 s Sprache, 1 s Stille → ein Seg, `start = 1.0s − 0.3s`, `end ≈ 3.0s + 0.3s` (±1 Frame).
  - `short_blip_is_ignored` — 0.1 s Sprache → nichts.
  - `long_speech_is_cut_at_the_quietest_frame` — 20 s Sprache-probs, Samples mit einem leisen Frame bei 13.5 s → erstes Seg endet dort (±1 Frame), zweites beginnt dort, keine Lücke, keins > 15 s.
  - `hysteresis_does_not_split_on_short_dips` — 0.3 s Dip unter SPEECH_OFF mitten in Sprache → ein Segment.
  - `two_hours_of_silence_keeps_the_buffer_small` — 2 h Stille in 1-s-Blöcken → `buffered() ≤ PAD + VAD_FRAME`, keine Segmente.
  - `open_segment_is_visible_while_speaking`.
- [ ] **Step 2:** FAIL. **Step 3:** Implementieren. **Step 4:** PASS; Mutationsprobe: Hysterese auf eine Schwelle reduzieren → `hysteresis…` rot; Kürzen des Leerlaufpuffers entfernen → `two_hours…` rot.
- [ ] **Step 5:** Commit `feat(transcribe-core): VAD-driven segmenter`.

### Task 5: Echo-Dedupe und Inferenz-Queue

**Files:**
- Create: `transcribe-core/src/dedupe.rs`, `transcribe-core/src/queue.rs`

**Interfaces:**
- Produces (`dedupe`):
  - `struct FinalSeg { source: Source, seq: u64, start_ms: u64, end_ms: u64, text: String, lang: String, confidence: f32 }`.
  - `struct EchoGate { threshold: f32, enabled: bool, held: VecDeque<(FinalSeg, u64 /*arrived_ms*/)>, recent_sys: VecDeque<FinalSeg> }`, `const OVERLAP_TOLERANCE_MS: u64 = 1000`, `const HOLD_MS: u64 = 3000`, `const SYS_MEMORY_MS: u64 = 30_000`.
  - `fn offer(&mut self, seg: FinalSeg, now_ms: u64) -> Vec<Release>`; `fn tick(&mut self, now_ms: u64) -> Vec<Release>`; `fn drain(&mut self) -> Vec<Release>`; `enum Release { Keep(FinalSeg), Drop(FinalSeg, String /*reason*/) }`.
  - Regel: SYS wird sofort `Keep` und gemerkt. MIC wird gehalten bis `HOLD_MS` vergangen ist **oder** ein SYS-Final eintrifft, dessen Ende ≥ MIC-Ende − Toleranz ist; dann `Drop`, wenn ein gemerktes SYS zeitlich überlappt (±Toleranz) und `similarity ≥ threshold`, sonst `Keep`. `enabled == false` → MIC sofort `Keep`.
  - Ähnlichkeit auch gegen **Konkatenation** zweier benachbarter SYS-Segmente prüfen (Segmentgrenzen fallen in den Streams unterschiedlich).
  - `fn headphones_suggest_off(transport: &str, data_source: Option<&str>) -> bool` — true bei Bluetooth/USB-Headset-Transport oder Datenquelle „Headphones“; built-in speakers/HDMI → false.
- Produces (`queue`):
  - `enum JobKind { Final, Hypothesis }`; `struct Job { kind, source, seq: u64, samples: Arc<Vec<f32>>, prompt: String, lang: Option<String> }`.
  - `struct JobQueue` mit `fn push(&mut self, job: Job)` (neue Hypothese ersetzt eine wartende Hypothese derselben Quelle), `fn pop(&mut self) -> Option<Job>` (Finals zuerst, FIFO innerhalb; dann Hypothesen, älteste Quelle zuerst — abwechselnd), `fn len(&self)`.

- [ ] **Step 1:** Tests:
  - `speaker_echo_on_mic_is_dropped` — SYS „Wir treffen uns um drei“ 10.0–12.0 s, MIC „wir treffen uns um drei“ 10.2–12.3 s → MIC `Drop`.
  - `own_speech_overlapping_sys_is_kept` — gleiche Zeit, anderer Text → `Keep`.
  - `mic_waits_for_late_sys` — MIC zuerst, SYS 1.5 s später → bis dahin keine Release für MIC; danach `Drop`.
  - `mic_is_released_after_hold_without_sys` — tick bei +3.1 s → `Keep`.
  - `echo_spanning_two_sys_segments_is_dropped`.
  - `disabled_gate_keeps_everything_immediately`.
  - `headphones_detection`: (`"bluetooth"`, None) → true; (`"builtin"`, Some("Internal Speakers")) → false; (`"builtin"`, Some("Headphones")) → true.
  - Queue: `finals_beat_hypotheses`, `newer_hypothesis_replaces_waiting_one`, `sources_alternate_between_hypotheses`.
- [ ] **Step 2:** FAIL. **Step 3:** Implementieren. **Step 4:** PASS; Mutationsproben: Konkatenations-Vergleich entfernen → `echo_spanning…` rot; Halte-Logik entfernen → `mic_waits…` rot.
- [ ] **Step 5:** Commit `feat(transcribe-core): echo gate and inference queue`.

## Phase 2 — Engine (whisper.cpp)

### Task 6: Modellkatalog und Download mit SHA-Prüfung

**Files:**
- Create: `transcribe-core/src/model.rs`

**Interfaces:**
- Produces: `struct ModelInfo { id: &'static str, file: &'static str, bytes: u64, sha256: &'static str }`, `const MODELS: &[ModelInfo]` (Werte aus Global Constraints; `id` = `large-v3-turbo-q5`, `large-v3-turbo-q8`, `medium-q5`, `small-q5`), `const DEFAULT_MODEL: &str = "large-v3-turbo-q5"`, `fn find(id: &str) -> Option<&'static ModelInfo>`, `fn models_dir() -> PathBuf`, `fn path_of(m: &ModelInfo) -> PathBuf`, `fn is_installed(m) -> bool` (Datei existiert **und** Größe stimmt — kein Voll-Hash bei jedem Start), `fn download(m, cancel: &AtomicBool, progress: impl FnMut(u64,u64)) -> Result<PathBuf, String>` (ureq-Stream in `<file>.part`, SHA-256 inkrementell, bei Abweichung `.part` löschen + Fehler `model.sha_mismatch`, sonst rename), `fn vad_model_path() -> Result<PathBuf, String>` (schreibt das eingebettete Silero-Modell einmalig nach `models_dir()`; vorher Größe+SHA gegen die Konstanten prüfen).
- Testbarkeit: `fn download_from(url: &str, …)` intern, damit Tests einen lokalen `std::net::TcpListener`-Server nutzen.

- [ ] **Step 1:** Tests: `catalogue_entries_are_unique_and_have_64_hex_sha`; `download_verifies_sha` (lokaler Mini-HTTP-Server liefert Bytes, SHA in Test-ModelInfo korrekt → Datei da, kein `.part`); `wrong_sha_removes_partial` (falscher SHA → Err `model.sha_mismatch`, weder `.part` noch Ziel existiert); `cancel_stops_download`; `embedded_vad_model_matches_its_hash` (include_bytes-Länge = 885098, sha = Konstante).
- [ ] **Step 2:** VAD-Modell nach `transcribe-core/models/ggml-silero-v6.2.0.bin` laden (curl von der URL in Global Constraints) und SHA prüfen; Lizenz-Hinweis in `transcribe-core/THIRD-PARTY.md`.
- [ ] **Step 3:** FAIL → implementieren → PASS.
- [ ] **Step 4:** Commit `feat(transcribe-core): model catalogue and verified download`.

### Task 7: Sprachwahl und Konfidenz (pure)

**Files:**
- Create: `transcribe-core/src/lang.rs`

**Interfaces:**
- Produces: `fn pick_de_en(probs: &[f32], de_id: usize, en_id: usize) -> &'static str` (größere der beiden; Gleichstand → `"de"`), `fn resolve_lang(setting: Lang, detected: Option<&str>, last: Option<&str>) -> String` (erzwungen gewinnt; sonst detected; sonst last; sonst `"de"`), `fn mean_logprob(token_probs: &[f32]) -> f32` (Mittel von `ln(p)`, p auf `1e-6` geklemmt; leer → `0.0`), `fn accept(text: &str, mean_lp: f32, no_speech: f32) -> Result<(), &'static str>` (Reihenfolge: leer/`is_noise_text` → `"noise"`; `mean_lp < -1.0` → `"low_confidence"`; `compression_ratio > 2.4` → `"loop"`; `no_speech > 0.6 && mean_lp < -0.5` → `"no_speech"`).

- [ ] **Step 1:** Tests: `english_wins_when_more_likely_even_if_shona_is_top` (probs mit Index 'sn' höchster, en > de → `"en"`); `forced_language_always_wins`; `hypothesis_uses_last_final_language`; Grenzwerte von `accept` je Regel.
- [ ] **Step 2–4:** FAIL → implementieren → PASS; Mutationsprobe: in `pick_de_en` das globale Maximum nehmen → erster Test rot.
- [ ] **Step 5:** Commit `feat(transcribe-core): de/en language pick and confidence rules`.

### Task 8: Engine über whisper-rs + Benchmark

**Files:**
- Create: `transcribe-core/src/engine.rs`, `transcribe-core/src/vad.rs`
- Modify: `transcribe-core/Cargo.toml` (whisper-rs nur unter `[target.'cfg(all(target_os = "macos", target_arch = "aarch64"))'.dependencies]`)

**Interfaces:**
- Produces (`engine`):
  - `struct Engine` (`WhisperContext`, `use_gpu(true)`, `flash_attn(true)`), `fn load(path: &Path) -> Result<Engine, String>`, `fn new_state(&self) -> Result<Stream, String>`.
  - `struct Stream` (eigener `WhisperState` + `last_lang: Option<String>`), `fn decode(&mut self, samples: &[f32], opts: &DecodeOpts) -> Result<Decoded, String>`.
  - `struct DecodeOpts { kind: JobKind, lang: Lang, prompt: String, threads: usize }`; Final: `SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 }`; Hypothese: `Greedy { best_of: 1 }`, `no_context(true)`, `single_segment(true)`, `audio_ctx` = `ceil(len_s * 50).clamp(256, 1500)` (nur Hypothesen). Für Finals mit `Lang::Auto`: zuerst `pcm_to_mel` + `lang_detect`, dann `pick_de_en`.
  - `struct Decoded { text: String, lang: String, confidence: f32 /*exp(mean_lp)*/, mean_logprob: f32, no_speech: f32 }` — `text` = Segmente per `drop_loops` zusammengefügt, Segmente einzeln per `accept` gefiltert.
  - Logging der whisper.cpp-Ausgabe auf `tracing` umleiten (`whisper_rs::install_logging_hooks`), sonst landet sie im stderr.
- Produces (`vad`):
  - `trait Vad { fn probs(&mut self, samples: &[f32]) -> Result<Vec<f32>, String>; }` (gibt `samples.len() / VAD_FRAME` Werte für die **neuen** Samples).
  - `struct SileroVad` (WhisperVadContext, CPU, 1 Thread) — ruft `detect_speech` auf einem Fenster aus 1 s Vorgeschichte + neuen Samples (der ggml-VAD setzt seinen Zustand pro Aufruf zurück; die Vorgeschichte wärmt ihn an) und gibt nur die Wahrscheinlichkeiten der neuen Frames zurück.

- [ ] **Step 1:** Testdaten: per `say -v Anna "…" -o … --data-format=LEI16@16000` und `say -v Samantha` je einen deutschen und englischen Satz plus 1 s Stille als `transcribe-core/tests/fixtures/{de,en,silence}.wav` erzeugen und committen (je < 300 KB).
- [ ] **Step 2:** Ignorierte Tests (brauchen das Modell, `#[ignore]`, Pfad über `IR_WHISPER_MODEL_PATH`): `transcribes_german` (Wort-Recall ≥ 0.9 gegen den Sollsatz), `transcribes_english`, `auto_picks_language`, `silence_yields_nothing`, `vad_marks_speech_frames` (Sprachframes > 0.5, Stille < 0.35).
- [ ] **Step 3:** Implementieren; einmalig `brew` hat `cmake` (vorhanden). Build: `cargo build -p transcribe-core --release`.
- [ ] **Step 4:** Modell per `download` holen und die ignorierten Tests laufen lassen: `IR_WHISPER_MODEL_PATH=… cargo test --release -p transcribe-core -- --ignored --nocapture` → PASS.
- [ ] **Step 5:** Benchmark-Test (ignoriert) `bench_decode` druckt ms für Final 15 s, Hypothese 5 s, Hypothese 10 s, je Modell. **Gate:** Hypothese ≤ 700 ms und Final 15 s ≤ 2500 ms auf dem M1 Pro mit `large-v3-turbo-q5`. Wenn nicht: Hypothesen mit `small-q5` als zweitem Modell im selben Prozess (eigener Context) — Entscheidung und Messwerte in den Commit-Text und CLAUDE.md.
- [ ] **Step 6:** Commit `feat(transcribe-core): whisper engine and silero VAD`.

## Phase 3 — Audioquellen und Pipeline

### Task 9: Resampler, Quellen-Abstraktion, Pipeline

**Files:**
- Create: `transcribe-core/src/resample.rs`, `transcribe-core/src/capture/mod.rs`, `transcribe-core/src/pipeline.rs`

**Interfaces:**
- Produces (`resample`): `struct Resampler` (`rubato::FftFixedIn` oder `SincFixedIn`, Eingang beliebige Rate/Kanäle), `fn new(in_rate: u32, channels: u16) -> Self`, `fn push(&mut self, interleaved: &[f32]) -> Vec<f32>` (16 kHz mono, Downmix = Mittel), `fn planar_push(&mut self, planes: &[&[f32]]) -> Vec<f32>`.
- Produces (`capture`): `trait AudioInput: Send { fn stop(self: Box<Self>); }`, `type Chunk = Box<dyn FnMut(&[f32]) + Send>` (16-kHz-mono-Samples), `fn start(source: Source, device: Option<&str>, on_chunk: Chunk, on_error: Box<dyn FnMut(String) + Send>) -> Result<Box<dyn AudioInput>, String>` (verzweigt auf Task 10).
- Produces (`pipeline`):
  - `struct Pipeline` mit `fn start(cfg: SessionConfig, sink: Arc<dyn Fn(Event) + Send + Sync>, deps: Deps) -> Result<Pipeline, String>`, `fn set_source(&self, source, on: bool)`, `fn window(&self, source, secs: u32) -> Option<Vec<i16>>` (für Shazam), `fn stop(self) -> Option<PathBuf>`.
  - `struct Deps { transcriber: Box<dyn Transcriber + Send>, vad: Box<dyn Fn() -> Box<dyn Vad + Send> + Send>, capture: Box<dyn CaptureFactory + Send> }` — `trait CaptureFactory { fn start(&self, source, device, chunk, err) -> Result<Box<dyn AudioInput>, String>; }` damit Tests Fake-Quellen und Fake-Engine einsetzen. `trait Transcriber { fn decode(&mut self, source: Source, samples: &[f32], opts: &DecodeOpts) -> Result<Decoded, String>; }` — die echte Implementierung hält ein `Engine` und je Quelle einen `Stream`.
  - Threads: je Quelle ein Thread (Chunk → Ringpuffer → VAD → Segmenter; alle 1 s Hypothesen-Job des offenen Segments, Fenster = letzte 10 s des offenen Segments); ein Inferenz-Thread (`JobQueue` pop → decode → für Hypothese `Agreement::update` → `Event::Hypothesis`; für Final → `EchoGate::offer` → Releases → Writer + `Event::Final`/`Dropped`; danach `Agreement::reset` der Quelle); ein Writer-Thread (Task 3). `EchoGate::tick` alle 250 ms im Inferenz-Thread.
  - `Event::Level` höchstens 10×/s je Quelle.
  - Quellen-Fehler: `SourceState{Lost}`, Neustart-Versuch alle 2 s (max. 30 Versuche), andere Quelle unberührt.
  - Prompt eines Final-Jobs = letzte 200 Zeichen des finalen Texts der Quelle; Hypothesen ohne Prompt.
  - Writer-`Fatal` → Pipeline stoppt, `Stopped{error}`.

- [ ] **Step 1:** Tests mit Fakes (Fake-VAD: Energie > 0.01 → 1.0; Fake-Transcriber: gibt vorgegebenen Text je Aufruf):
  - `resampler_48k_stereo_to_16k_mono` (1 kHz Sinus 48 kHz stereo → Länge ±1 %, Frequenz bleibt: Nulldurchgänge zählen).
  - `two_sources_are_labelled_separately` — Fake-MIC und Fake-SYS mit je einem Satz → zwei `Final` mit richtiger Quelle, zwei Zeilen im Transkript.
  - `echo_is_not_written_twice` — beide Quellen liefern denselben Text zur selben Zeit → eine Zeile (SYS).
  - `hypotheses_never_reach_the_file`.
  - `lost_source_does_not_stop_the_other` — Fake-MIC ruft `on_error` → `SourceState{mic, Lost}`, SYS-Finals laufen weiter.
  - `stop_flushes_open_segment_and_completes_session` — Status `Completed`.
  - `fatal_writer_error_stops_the_session`.
- [ ] **Step 2–4:** FAIL → implementieren → PASS.
- [ ] **Step 5:** Commit `feat(transcribe-core): two-source pipeline`.

### Task 10: macOS-Quellen — cpal-Mikrofon, Process Tap, ScreenCaptureKit-Fallback, App Nap

**Files:**
- Create: `transcribe-core/src/capture/mic.rs`, `transcribe-core/src/capture/tap_macos.rs`, `transcribe-core/src/capture/sck_macos.rs`, `transcribe-core/src/activity_macos.rs`
- Read for reuse: `core/rust-lib/src/mic_capture.rs` (cpal-Geräte/Formate), `core/rust-lib/src/system_audio.rs` (SCK-Audio, `to_mono`), `core/rust-lib/src/boom/macos.rs:371-460` (`make_tap_description`, `build_aggregate` — Tap-Erzeugung, hier mit `muteBehavior = unmuted`, eigener UID `io.celox.inspector-rust.transcribe`, Name „InspectorRust Transkription“)

**Interfaces:**
- `mic::start(device: Option<&str>, …)` — Gerät per Name, sonst Standard; jede Sample-Art (F32/I16/U16) → f32 → `Resampler`.
- `fn list_input_devices() -> Vec<String>`.
- `tap_macos::start(…)` — `CATapDescription initStereoGlobalTapButExcludeProcesses:@[]`, `setPrivate:YES`, `AudioHardwareCreateProcessTap`, privates Aggregate mit Tap + aktueller Standardausgabe als Haupt-Subdevice, IOProc → Kopie in lock-freien Ringpuffer → Verarbeitungsthread → `Resampler` (Tap-Format über `kAudioTapPropertyFormat` lesen). Aufräumen: IOProc stoppen/zerstören, Aggregate zerstören, `AudioHardwareDestroyProcessTap`. Ändert sich die Standardausgabe (Listener auf `kAudioHardwarePropertyDefaultOutputDevice`): Aggregate neu bauen.
- `sck_macos::start(…)` — Port aus `system_audio.rs` (kein Rewrite, Code verschieben/kopieren; das alte Modul bleibt bis Task 16).
- `capture::start(Source::Sys, …)` versucht Tap; scheitert er (OS < 14.2, `!= noErr`) → SCK + `Warning{code:"sys.fallback_sck"}`.
- `fn headphones_hint() -> Option<bool>` — Standardausgabe: Transport (`kAudioDevicePropertyTransportType`) + Datenquelle (`kAudioDevicePropertyDataSource` Name) → `dedupe::headphones_suggest_off`.
- `activity_macos::Activity::begin(reason: &str) -> Activity` (`NSProcessInfo beginActivityWithOptions:reason:` mit `NSActivityUserInitiated`), `Drop` → `endActivity`.

- [ ] **Step 1:** Ignorierte Live-Tests: `mic_live_produces_samples` (3 s, RMS > 0), `tap_live_hears_say` (startet Tap, `say "test eins zwei"`, RMS > 0.005 während der Ausgabe), `sck_fallback_live`.
- [ ] **Step 2:** Implementieren; jedes `unsafe` mit `// SAFETY:`.
- [ ] **Step 3:** Live-Tests laufen lassen (`cargo test -p transcribe-core --lib capture -- --ignored --nocapture`); Testbinary braucht Mikrofon-/Audio-Capture-Freigabe (TCC-Prompt bestätigen).
- [ ] **Step 4:** `cargo clippy -p transcribe-core -- -D warnings`.
- [ ] **Step 5:** Commit `feat(transcribe-core): mic, process tap and SCK capture`.

## Phase 4 — Helper und App-Schicht

### Task 11: `ir-transcribe`-Helper

**Files:**
- Modify: `transcribe-helper/src/main.rs`; Create: `transcribe-helper/src/serve.rs`
- Create: `scripts/bundle-transcribe.sh` (baut `cargo build --release -p transcribe-helper`, kopiert nach `$1/Contents/MacOS/ir-transcribe`, `xattr -c`; für Nicht-arm64-Triples No-op)
- Modify: `scripts/install-macos.sh`, `scripts/pack-macos-release-dmg.sh` (je **eine** Zeile: `bundle-transcribe.sh` vor dem Signieren aufrufen; `NSAudioCaptureUsageDescription` per `plutil -replace` neben dem Mikrofon-Text, Text: „Inspector Rust transkribiert den Systemton nur, während du eine Transkription startest.“)

**Interfaces:**
- `fn serve(input: impl BufRead, output: impl Write)` — eine Zeile = ein `Command`; `Ready` als erstes Event; Events aus allen Threads über einen Kanal serialisiert geschrieben (ein Writer-Thread hält stdout); Modell wird erst bei `Start`/`TranscribeFile` geladen und bleibt bis `Shutdown`/EOF; nur eine Session/Datei-Job zur Zeit (`Error{"transcribe.busy"}`).
- EOF auf stdin oder `Shutdown` → laufende Session `stop()` (Status `Completed`), `Stopped`, Prozessende.
- Panics in Commands: `catch_unwind` → `Error`; whisper.cpp-`abort` beendet nur den Helper.
- `Activity::begin` für die Dauer einer Session/Datei.

- [ ] **Step 1:** Integrationstest `tests/helper_protocol.rs` (startet das gebaute Binary über `env!("CARGO_BIN_EXE_ir-transcribe")`): `ready_first`, `unknown_command_reports_error_and_keeps_running`, `eof_ends_the_process_within_2s`, `start_without_model_reports_model_missing` (Fehler `model.missing`).
- [ ] **Step 2–3:** FAIL → implementieren → PASS.
- [ ] **Step 4:** `scripts/bundle-transcribe.sh` in einem Wegwerf-Bundle-Ordner probieren.
- [ ] **Step 5:** Commit `feat(transcribe): ir-transcribe helper`.

### Task 12: App-Client, Meeting-Zustand, Commands, Recovery beim Start

**Files:**
- Create: `core/rust-lib/src/transcribe_client.rs` (Helper-Prozess), `core/rust-lib/src/meeting.rs` (Zustand + Settings + Tauri-Glue)
- Modify: `core/rust-lib/Cargo.toml` (`transcribe-protocol` path dep), `core/rust-lib/src/lib.rs` (mod, invoke_handler, Recovery im setup, Shutdown bei `RunEvent::Exit`), `core/rust-lib/src/commands.rs` (Wrapper)
- Test: in den Modulen

**Interfaces:**
- `transcribe_client`: `fn send(cmd: Command) -> Result<(), String>` (spawnt bei Bedarf `Contents/MacOS/ir-transcribe`; fehlt er → `transcribe.no_helper`), `fn subscribe(f: Box<dyn Fn(Event) + Send + Sync>)`, Reader-Thread verteilt Events; stirbt der Helper während einer Session → synthetisches `Stopped{error:"transcribe.helper_died"}`; `fn shutdown()`.
- `meeting`:
  - `struct MeetingConfig` (Settings-Keys `meeting.sources` JSON, `meeting.mic_device`, `meeting.model`, `meeting.lang`, `meeting.root`, `meeting.keep_audio`, `meeting.audio_days`, `meeting.dedupe`, `meeting.dedupe_threshold`, `meeting.ticker` JSON `TickerConfig`), `fn load(db) -> Self`, `fn save(&self, db)`, `fn normalized(self) -> Self` (Schwelle 0.5–0.95, Tage 1–365, Modell aus Katalog sonst Default).
  - `struct TickerConfig { visible: bool, position: "top"|"bottom", display: Option<String>, width_pct: u8 /*30–100*/, font_px: u8 /*11–28*/, opacity: f32 /*0.3–1.0*/, speed_px_s: u16 /*20–200*/ }` mit Defaults (true, bottom, None, 70, 15, 0.85, 60).
  - `struct MeetingState { running: bool, dir: Option<PathBuf>, sources: BTreeMap<Source, SourceView>, lines: VecDeque<LineView> /*max 200*/, hypotheses: BTreeMap<Source, (String,String)>, warnings: Vec<String>, started_ms: Option<u64> }`, `fn apply(&mut self, ev: &Event)` (pure, getestet), Tauri-Event `meeting-changed` (gedrosselt ≤ 10/s), `meeting-line` für jedes Final, `meeting-hypothesis` für Hypothesen (beide für den Ticker).
  - IPC (alle `async` + `spawn_blocking` wo sie den Helper anfassen): `meeting_start(sources?: Vec<Source>)`, `meeting_stop`, `meeting_status`, `meeting_set_source(source, on)`, `meeting_sessions` (→ `list_sessions`), `meeting_open_dir(path)`, `get_/set_meeting_config`, `model_status` (installiert/Größe je Katalog-Modell), `model_download(model)`, `model_cancel_download`.
  - Vor `Start`: Mikrofon-Berechtigung prüfen (`AVCaptureDevice authorizationStatusForMediaType:`), bei `denied` → `transcribe.mic_denied` + Link `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone`; Audio-Capture-Denial meldet der Helper als `SourceState{sys, Lost, "permission"}` → Link `…?Privacy_AudioCapture`.
  - Start-Recovery (setup, Worker-Thread): `recover(root)` + Fallback-Root; ≥ 1 Treffer → Status-Toast (Kind `transcribe`) „N Transkription(en) abgebrochen — gesichert bis HH:MM“ und Eintrag in `meeting_status().recovered`.
  - Busy-Regel gegen `live_transcribe`/`transcribe`-Job: gegenseitig `transcribe.busy`.
- [ ] **Step 1:** Tests: `apply_keeps_last_200_lines`, `hypothesis_is_replaced_by_final_of_same_source`, `config_normalizes_out_of_range_values`, `config_defaults_when_unset`, `recovery_at_start_marks_and_reports` (Tempordner mit `running`-Session).
- [ ] **Step 2–4:** FAIL → implementieren → PASS; `cargo build -p inspector-rust-core`.
- [ ] **Step 5:** Commit `feat(meeting): helper client, state and commands`.

### Task 13: Tray, Hotkeys, Aufnahme-Indikator

**Files:**
- Modify: `core/rust-lib/src/lib.rs` (`build_tray`), `core/rust-lib/src/hotkey.rs` (`ActionId`), `scripts/gen-tray-icon.py` (Variante), Create: `core/rust-lib/assets/tray-hat-rec.png`

**Interfaces:**
- `ActionId::MeetingToggle` (Key `meeting_toggle`, Default `Ctrl+Shift+Alt+KeyL`), `ActionId::TickerToggle` (`ticker_toggle`, `Ctrl+Shift+Alt+KeyK`); `ALL` auf 15, Count-Pins anpassen; Dispatch auf Worker-Thread → `meeting_start`/`stop` bzw. Ticker-Sichtbarkeit; Status-Toast beim Umschalten.
- Tray-Menü: „Transkription starten/stoppen“, Unterpunkte „Mikrofon“/„Systemton“ (Check), „Laufband anzeigen“ (Check), „Transkripte öffnen“. Bei laufender Session: Icon `tray-hat-rec.png` **nicht** als Template (roter Punkt muss rot sein), Tooltip „Transkription läuft“; bei Writer-Warnung Tooltip ergänzt. Nach Stopp zurück auf Template-Icon.
- [ ] **Step 1:** Test `meeting_actions_have_unique_free_defaults` (gegen alle anderen Defaults via `find_conflict`); bestehender Count-Pin auf 15.
- [ ] **Step 2–4:** Implementieren, `cargo test -p inspector-rust-core hotkey`, PASS.
- [ ] **Step 5:** Commit `feat(meeting): tray, hotkeys and recording indicator`.

## Phase 5 — Ticker

### Task 14: Ticker-Fenster und -Rendering

**Files:**
- Create: `core/rust-lib/src/ticker.rs`; `core/frontend/src/components/TranscribeTicker.tsx`; `core/frontend/src/lib/ticker.ts` (+ `ticker.test.ts`)
- Modify: `core/frontend/src/main.tsx` (Route `transcribe-ticker`), alle drei `capabilities/default.json` (Fenster `transcribe-ticker`)

**Interfaces:**
- Rust: `fn show(app, cfg: &TickerConfig)`, `fn hide(app)`, `fn apply(app, cfg)` — Bau auf Worker-Thread → `run_on_main_thread`; `transparent`, `decorations(false)`, `focused(false)`, `skip_taskbar`, `always_on_top`, `set_ignore_cursor_events(true)`; macOS: `setLevel: NSScreenSaverWindowLevel`(1000), `setCollectionBehavior: canJoinAllSpaces|stationary|fullScreenAuxiliary|ignoresCycle`, `setCanHide:NO`, `setHasShadow:NO`, Anzeige per `orderFrontRegardless`; Geometrie: Bildschirm aus `cfg.display` (Name) sonst Cursor-Monitor, Breite `width_pct`, Höhe `font_px * 2.2`, oben/unten mit 8 px Abstand, physische Pixel.
- TS (`lib/ticker.ts`, pure):
  - `type Piece = { id: string; source: "mic"|"sys"; text: string; final: boolean }`.
  - `function applyFinal(pieces, ev): Piece[]` — ersetzt das vorläufige Piece derselben Quelle durch ein finales (gleiche Position im Strom = am Ende angehängt, vorläufiges entfernt), max. 40 Pieces.
  - `function applyHypothesis(pieces, ev): Piece[]` — genau ein vorläufiges Piece je Quelle am Ende; Text = `stable + " " + pending`.
  - `function advance(offsetPx, dtMs, speedPxS, backlogPx): number` — Grundgeschwindigkeit, bei Rückstau (Text mehr als 1,5 Bildschirmbreiten rechts außerhalb) bis 3× schneller, damit die Verzögerung nicht wächst.
  - `function prune(pieces, widths, offsetPx): { pieces, offsetPx }` — links herausgelaufene Pieces entfernen und `offsetPx` um deren Breite korrigieren (kein Sprung).
- `TranscribeTicker.tsx`: eine rAF-Schleife, bewegt einen Container nur per `transform: translateX`, Breiten per `ResizeObserver` einmal pro Piece gemessen; Final `color: #f5f5f5`, vorläufig `#9a9a9a` kursiv; Label-Chip `MIC` (Akzent `#38bdf8`) / `SYS` (`#f59e0b`); Deckkraft aus Config; hört auf `meeting-line`, `meeting-hypothesis`, `meeting-ticker-config`; `prefers-reduced-motion` → statisch letzte Zeile.
- [ ] **Step 1:** Tests `ticker.test.ts`: `final_replaces_pending_of_same_source_only`, `one_pending_piece_per_source`, `prune_keeps_visual_position` (Summe offset + entfernte Breiten konstant), `backlog_speeds_up_up_to_3x`, `caps_at_40_pieces`.
- [ ] **Step 2–4:** FAIL → implementieren → PASS (`pnpm --filter inspector-rust-frontend exec vitest run src/lib/ticker.test.ts`).
- [ ] **Step 5:** Commit `feat(meeting): ticker overlay`.

## Phase 6 — Oberfläche

### Task 15: `meeting`-Panel, Settings-Sektion, Doku

**Files:**
- Create: `core/frontend/src/components/MeetingPanel.tsx` (lazy), `core/frontend/src/components/MeetingSection.tsx` (Settings), `core/frontend/src/lib/meeting.ts` (+ Test)
- Modify: `core/frontend/src/lib/commands.ts` (CommandSpec `meeting`, Alias `mitschrift`, `platform: ["mac"]`, `description_de`), `App.tsx` (`PANEL_KINDS`, `commandEntry`-case, show-while-typed, Enter = Start/Stop), `lib/commandDocs.ts` (CommandDoc ≥ 3 Beispiele, Tips/Caveats), `lib/ipc.ts`, `lib/settings-sections.ts` (Eintrag `transcription` ans Ende + Kategorie), `SettingsPanel.tsx` (Block + Footer `© {Jahr} Martin Pfeffer | celox.io`), `locales/en.json` + `de.json`, `features.txt`, `CHANGELOG.md`, `README.md`/`README.de.md` via `node scripts/gen-docs.mjs`, `docs/meeting.md` (neu, im Doku-Index beider READMEs), `CLAUDE.md` (Abschnitt „meeting — Live-Transkription MIC+SYS“)
- **Interfaces:** `lib/meeting.ts`: `formatElapsed(ms)`, `sourceLabel(s)`, `sessionTitle(meta)`, `modelSizeLabel(bytes)`.
- Panel: Start/Stop-Knopf, Quellen-Schalter (MIC/SYS), Pegelbalken je Quelle, Live-Liste der letzten Zeilen (Final hell, Hypothese grau kursiv), Warnungen, „Modell fehlt → herunterladen (574 MB)?“-Karte mit Fortschritt, Berechtigungshinweise mit Direktlink, Liste vergangener Sessions (abgebrochen markiert, Klick = im Finder zeigen).
- Settings: Modell (Auswahl + Status + Download), Sprache, Mikrofon-Gerät (`Command::ListDevices` → `Event::Devices`, Helper ruft `mic::list_input_devices`), Speicherpfad (Ordnerwahl), Rohaudio + Tage, Dedupe-Modus + Schwelle, Ticker-Optik (Position, Bildschirm, Breite, Schrift, Deckkraft, Tempo), Hinweis auf Global shortcuts.
- [ ] **Step 1:** Tests: `meeting.test.ts` (Formatierer), `commandDocs`/`commands`/`i18n`/`settings-sections`/`readme-badges`-Gates grün; `MeetingPanel.test.tsx`: Enter startet, zweites Enter stoppt; Modell fehlt → Karte, kein Start.
- [ ] **Step 2–4:** Implementieren; `bash scripts/check.sh` und `pnpm test` grün; App-Chunk ≤ 400 KB (`node scripts/check-bundle.mjs`).
- [ ] **Step 5:** Commit `feat(meeting): panel, settings and docs`.

## Phase 7 — Alte Befehle auf die neue Engine

### Task 16: Diktat, Song/Raum und Datei-Modus über den Helper, Python entfernen

**Files:**
- Modify: `core/rust-lib/src/live_transcribe.rs` (Worker-/PCM-Datei-Teil ersetzen durch `transcribe_client` mit `Mode::Dictation|Song|Room`; `LiveJob`, Shazam-Logik, Ducking, `result_text`/`lyrics_clip_text` bleiben; Shazam-Fenster über `Command::Window`), `core/rust-lib/src/transcribe.rs` (Datei-Job über `Command::TranscribeFile`; TXT-Benennung `free_txt_path` bleibt), `transcribe-core/src/file.rs` (neu: AVAssetReader → 16 kHz mono f32, dann Segmentierung + Finals → TXT, `FileProgress`)
- Delete: `core/rust-lib/assets/transcribe/live.py`, `runner.py`, `scripts/setup-transcribe.py`, Python-Pfad-Funktionen, die Python-Tests darin
- Modify: Doku (`docs/transcribe.md`, CLAUDE.md-Abschnitte „Local media transcription“ + „Live dictation“, CommandDoc `transcribe`, features.txt, CHANGELOG), `commands.ts` `transcribe` → `platform: ["mac"]`
- **Interfaces:** Mode-Mapping: `Dictation` = nur Mic, Song = nur Sys, Room = Mic mit 20-s-Max-Segment (Segmenter-Konstante pro Mode: `max_seg_for(mode)`), kein Dateischreiben für diese drei Modi (`root` = privater Cache-Ordner, nach Ende gelöscht), Ergebnis = verbundener Finaltext.
- [ ] **Step 1:** Bestehende pure Tests in `live_transcribe.rs`/`transcribe.rs`, die weiter gelten (Job-Zustand, `result_text`, Benennung), bleiben grün; Python-spezifische Tests entfernen; neuer Test `file_job_maps_progress_and_done_events`.
- [ ] **Step 2:** Ignorierter Live-Test: Datei-Modus auf `tests/fixtures/de.wav` und einer m4a (per `afconvert` erzeugt) → TXT enthält den Satz.
- [ ] **Step 3:** Implementieren, `cargo test --workspace --exclude openscreen-compositor`, `pnpm test`, `bash scripts/check.sh`.
- [ ] **Step 4:** Commit `refactor(transcribe): run dictation, song and file mode on the new engine; drop Python`.

## Phase 8 — Abnahme

### Task 17: Installieren, Dauerlauf, Crash-Test, Merge-Vorbereitung

- [ ] **Step 1:** `scripts/install-macos.sh` aus dem Worktree (erst wenn die Aufnahme-Session nicht gerade installiert — vorher fragen); Helper im Bundle prüfen (`ls Contents/MacOS/ir-transcribe`, `otool -L` ohne Homebrew-Pfade).
- [ ] **Step 2:** Live: Meeting starten, YouTube-Video über Lautsprecher + eigene Stimme (de und en) → Transkript beschriftet, keine Doppelzeilen, Ticker ≤ 3 s (Stoppuhr am Bildschirm, Messwert notieren).
- [ ] **Step 3:** `kill -9` auf App-Prozess während Session → Helper endet ≤ 2 s, Session `completed` oder (bei `kill -9` auf den Helper) beim nächsten Start `aborted` + Toast; Transkript enthält alle Zeilen bis auf das laufende Segment.
- [ ] **Step 4:** 2-Stunden-Lauf im Hintergrund (Hauptfenster zu, Video-Playlist): RSS von App und Helper alle 10 min loggen (`ps -o rss= -p …` in eine Datei im Scratchpad); Kriterium: Helper-RSS nach der ersten Stunde wächst < 50 MB/h.
- [ ] **Step 5:** `git fetch`/Rebase auf aktuellen `main` (erst wenn die Aufnahme-Session committet hat), Konflikte in `lib.rs`/`hotkey.rs`/`Cargo.lock`/Doku lösen, volle Suiten + Badges (`pnpm test`), Version bump + CHANGELOG, Commit, Bericht mit Messwerten an den Nutzer. Push/Merge nach Rückfrage.
