# Stage 6 recon — captions and transcript editing

Read-only; nothing in the clone was executed. Prefixes: `V/` = this folder, `C/` =
`<scratchpad>/openscreen-src/new/` (untrusted upstream clone), `IR/` = repo root,
`WR/` = `~/.cargo/registry/src/index.crates.io-*/whisper-rs-0.16.0/`.
Upstream design doc: `C/technical-documentation/architecture/transcription-and-captions.md`
(where it disagrees with the code, the code wins — see §6).

## 0. Summary

1. **Captions are derived, never stored.** Each render groups the per-asset word list
   (`document.transcripts[]`, source seconds) into lines and projects them through every
   clip that plays that asset (`C/src/lib/ai-edition/captions/cues.ts:1-15`). The only
   caption edit is editing a word's text — no timing edit, no split/merge
   (`transcription-and-captions.md:638-647`).
2. **Rendering needs no native code.** Captions are synthetic text annotations:
   `space:"frame"`, `text.verticalAlign:"bottom"|"top"`, `textAnimation:"none"`,
   `zIndex = 100000 + i` (`cues.ts:51,299-334`, `C/src/native/sceneDescription.ts:1119-1147,1575-1621`).
   `V/src/scene.rs:493-577` parses `space`, `verticalAlign`, `animation`;
   `V/src/text_macos.rs:340-380` pins the block to the edge; `rgba()` plates parse
   (`V/src/frame_geometry.rs:8265-8270`). Same path for preview and export.
3. **IR `scene_json.rs` emits text annotations with `space:"frame"`** (`IR/core/rust-lib/src/record/scene_json.rs:613-644`)
   but never `verticalAlign`, and has no caption source.
4. **No karaoke / current-word highlight upstream** (only the transcript pane highlights the cue word,
   `C/src/components/ai-edition/RightPanes.tsx:1121-1153`).
5. **Upstream transcript cut = trim range**, not word removal (§3). IR has no trim ranges;
   its cut is `splitAt` + `deleteClip`, which truncates zooms/annotations spanning the cut
   (`IR/core/frontend/src/lib/editor.ts:81-92`).
6. **Our whisper file path returns plain text only** (`Command::TranscribeFile` → `FileDone{txt, lang}`,
   `IR/transcribe-protocol/src/lib.rs:118,163-168`); `transcribe_stream` drops each
   segment's start (`IR/transcribe-core/src/file.rs:127`). `Engine::load` uses
   `flash_attn(true)` (`IR/transcribe-core/src/engine.rs:30`), which disables DTW
   (`WR/src/whisper_ctx.rs:464`).

## 1. Upstream transcription

- whisper.cpp v1.9.1 HTTP helper `whisper-stt-server` in the Electron main process;
  ffmpeg decodes to mono 16 kHz (`C/electron/stt/extractAudio.ts`).
- Model `ggml-small-q8_0.bin` (pinned HF revision, SHA-256 checked every start,
  `C/electron/stt/modelManager.ts:61`), Silero VAD v6.2.0 (`:70`); optional wav2vec2 CTC aligners (en/fr).
- Decode (`C/electron/native/whisper-stt/src/main.cpp`): `flash_attn=false` (`:386`, marked CRITICAL),
  `dtw_token_timestamps=true`, `dtw_aheads_preset=WHISPER_AHEADS_SMALL` (`:390-391`), greedy (`:509`),
  `token_timestamps=true` (`:510`), language auto or forced (`:511`), glossary prompt (`:520-521`).
  Any `t_dtw == -1` or non-monotonic `t_dtw` fails the request (`:589-646`).
- VAD is cut by the helper itself and times mapped back, because whisper.cpp's `vad`
  param maps segment but not token times (`main.cpp:525-526`).
- **`t_dtw` is a token's END:** `word.start = t_dtw of the previous text token`,
  `word.end = t_dtw of the word's last token` (`main.cpp:605-609`; using the first token's own
  `t_dtw` made words ~175 ms late). Tokens join into a word at a leading space.
- ~90 s chunks cut at the quietest frame (`C/electron/stt/chunking.ts:50`).
- Wire: `{ segments:[{text,startSec,endSec}], wordSegments:[{word,startSec,endSec,confidence?}], detectedLanguage }`,
  absolute source seconds (`C/electron/stt/transcriptionContract.ts:17-30,150-161`).
- Stored (`C/src/lib/ai-edition/schema/index.ts`): `transcript = {assetId, language, segments[], words[]}` (`:111-118`),
  one per asset; `word = {id, segmentId, startSec, endSec, text, originalText?, source?: asr|user|synth}` (`:67-95`).
- Word text edit: `setWordText` (`C/src/lib/ai-edition/document/transcript.ts:65-113`) stores `originalText` on the
  first edit; typing the original back clears it; empty text hides the word from captions (`cues.ts:55`); one undo step.
- Re-transcription keeps user edits: `carryOverWordEdits` matches same `originalText` with an overlapping span
  (`transcript.ts:224-265`).
- Word insertion is DEV-only — do not port.

## 2. Caption derivation and rendering

1. Lines (`C/src/lib/captioning/annotationsFromCaptions.ts:210-307`): a run of words breaks at a gap
   **≥ 0.24 s** (`WORD_RUN_BREAK_GAP_SEC`, `:16`); each run is cut into lines of min..max words
   (`computeCaptionLineIndexRanges`, avoids a too-short tail); line = `[first.start, max(first.start+0.02, last.end)]`,
   words joined by one space, overlaps trimmed.
2. Dedupe: same normalized text with gap < 0.55 s or overlap > 15 ms is merged (`:19,66-87`).
3. Projection (`cues.ts:162-186`): each line through every clip of its asset (duplicated clips repeat it; a line split
   by a clip edge gives several spans). Overlapping cues shortened (`:249-257`). Empty when `enabled` false.
4. Region per cue (`cues.ts:299-334`): box from `captionBoxRect`, `space:"frame"`, `verticalAlign`, style
   `{color, backgroundColor: rgba(...), fontSize, fontFamily, fontWeight, textAlign: anchorH, textAnimation:"none"}`,
   `fontSizeRel = fontSize/1080`.
5. Box (`C/src/lib/ai-edition/captions/settings.ts:309-311,377-404`), percent of the output frame:
   column `aspect ≥ 1.5 → {x:16,width:68}` else `{x:5,width:90}`;
   height `clamp(fontSize × (3 × 1.5 + (plate ? 0.2 : 0)) / 1080 × 100, 10, 100 − insetY)`;
   width centre = column width, else `min(column.width, 100 − insetX)`;
   x left `insetX`, right `100 − insetX − width`, centre `(100 − width)/2`;
   y bottom `100 − insetY − height`, top `insetY`.
6. Native: `SceneAnnotation.space` (`V/src/scene.rs:511,537-544`), `SceneAnnotationText{…, verticalAlign?, animation?}`
   (`:549-579`); macOS `block_layout` pins top/bottom (`V/src/text_macos.rs:340-380`); plate padding 0.2 × 0.1 em
   (`V/src/text_plate.rs:21-32`); text cache keyed by annotation id + spec (`V/src/compositor_macos.rs:2156-2160`);
   per-clip gating by clip index + source seconds (`V/src/scene.rs:1060-1062`) — speed applies automatically.
   **No vendor patch needed.**

## 3. Upstream transcript cut

- `timeline.trimRanges[] = {id, assetId, clipId?, startSec, endSec, reason, origin}`, source seconds per clip
  (`schema/index.ts:264-279`); clips are not split; words stay in the transcript, shown struck through.
- Gesture (`RightPanes.tsx:1168-1255`): Backspace/Delete on the caret word or a selection of kept words → span
  `[min start, max end]` → `breatheCut` → spread over the clips under it (`NewEditorShell.tsx:727-768`).
- `breatheCut` (`C/src/lib/ai-edition/timeline/cut-breath.ts:33-70`): an edge sitting in a gap next to a **kept**
  word is pulled back to leave `CUT_BREATH_SEC = 0.07` of the gap, capped at the gap's midpoint; an edge inside a
  word or next to removed material is left alone. Hand-drawn trims are never breathed (`:9-10`).
- `mergeCloseCuts` (`cut-breath.ts:78-88`): snap an edge onto a neighbouring cut when < 2 frames would remain
  (fallback 30 fps).
- Restore: clicking the struck-through run removes the whole cut (`RightPanes.tsx:1201-1210`).
- Pane: one block per clip; `[silence]` pseudo-words for gaps ≥ 0.2 s (`aggregated-transcript.ts:71-72`).
- Regions (zoom/annotation/layout) are clip-anchored in source time and survive cuts (`timeline-model.md:231-257`).

## 4. Timing model

Transcript: source seconds per asset; cues derived every render, so split/delete/reorder/duplicate/speed need no
caption update. Each take/asset has its own transcript.

## 5. IR state

- `EDITOR_VERSION = 4` (`IR/core/rust-lib/src/record/editor.rs:15`), `migrate` v1→v4 (`:904-933`), `EditDoc` (`:746-781`):
  every region = clip index + source ms of that clip's take; `MIN_CLIP_MS = 100`; `TEXT_FONTS` = upstream's five;
  `MAX_Z_INDEX = 1 000 000` (`:73`).
- TS `splitAt` gives a spanning zoom/annotation to the half holding its start and clamps its end
  (`IR/core/frontend/src/lib/editor.ts:81-92`); speed splits properly (`:71-80`).
- Audio: `mic.wav` with `MicMeta{offset_ms}`; **mic time = source time − offset** (`scene_json.rs:253,310,934`);
  takes under `<project>/takes/<id>/` with their own `mic_offset_ms` (`editor.rs:977-980`).
- Engine: whisper-rs 0.16 (bundles whisper.cpp 1.8.3), Metal; `Lang` de/en/auto; final decode beam 5,
  `no_context`, prompt = last 200 chars; −50 dBFS silence never decoded; Silero VAD embedded; own segmenter
  (`IR/transcribe-core/src/segment.rs:11-25`, `Seg{start,end}` = sample indices); file mode Core Audio then ffmpeg.
  Helper refuses with `ERR_BUSY` during a meeting / another file (`IR/transcribe-helper/src/serve.rs:283-287`).
- Word timings need: a second `WhisperContext` with `flash_attn(false)` +
  `dtw_parameters(DtwParameters{mode: DtwMode::ModelPreset{model_preset: DtwModelPreset::LargeV3Turbo}, ..})`
  (`WR/src/whisper_ctx.rs:469-530,612-643`), `FullParams::set_token_timestamps(true)` (`WR/src/whisper_params.rs:189`),
  `token_data().t_dtw` (centiseconds), BPE grouping at a leading space, start/end rule from §1, segment offset added
  back, `t_dtw == -1` handled. Upstream's char-level DTW and CTC aligner are not available through whisper-rs, so
  timing is coarser.
- Models: `~/Library/Application Support/InspectorRust/models/` (`IR_TRANSCRIBE_MODELS_DIR`), default
  `large-v3-turbo-q5` (574 041 195 B); also `large-v3-turbo-q8`, `medium-q5`, `small-q5`.

## 6. Upstream defaults (verbatim, `settings.ts:107-127`)

| field | default | range |
|---|---|---|
| enabled | false | |
| fontSize | 48 (px at 1080) | pane 16–140 (`:130-131`), read clamp 12–200 |
| fontFamily | Inter | Inter, Lora, Oswald, Caveat, IBM Plex Mono |
| fontWeight | bold | normal, bold |
| color | #ffffff | |
| backgroundEnabled | true | opacity < 0.2 reads as off |
| backgroundColor | #000000 | named plates dark #000000, light #ffffff |
| backgroundOpacity | 0.55 | 0.4–0.9 |
| anchorV | bottom | bottom, top |
| insetY | 1.5 (aspect ≥ 1.5) / 12.5 | 0–50, step 0.5 |
| anchorH | center | left, center, right |
| insetX | 10 / 5 | 0–25, step 0.5 (ignored for centre) |
| min/maxWordsPerLine | 2 / 7 | 1–12, min ≤ max |

Constants: reference height 1080, line height 1.5 em, plate pad 0.1 em, 3 box lines, landscape aspect 1.5,
z base 100000, run break 0.24 s, dedupe gap 0.55 s, min line span 0.02 s, cut breath 0.07 s, silence 0.2 s,
merge close cuts 2 frames.

Looks (`settings.ts:186-224`): classic Inter bold 48 #ffffff dark 0.55 · bold Oswald bold 60 #fde047 dark 0.85 ·
minimal Inter normal 40 #ffffff dark 0.4 · light Inter bold 48 #111111 light 0.9. Choosing a plate flips the text
colour (`captionPlatePatch`, `:156-169`). The doc says `insetY` default 5; the code says 1.5 — use the code.

## 7. Risks

1. Cutting by split truncates spanning regions (fix: duplicate them into both halves).
2. `MIN_CLIP_MS = 100`: a cut near a clip edge must become a trim of that edge.
3. Captions must be derived, not stored as annotations.
4. Source clock: `source_ms = mic_ms + mic.offset_ms`, per take.
5. `t_dtw` is the token END.
6. `flash_attn(true)` silently disables DTW; helper serves one job at a time.
7. Loop/repeat filters work on strings — filter the word list the same way.
8. Without `verticalAlign` captions centre in their box and jump when they wrap.
9. Real annotations may use z ≥ 100 000 — sort captions last.
10. Captions per clip window, not per take; clamp spans crossing clip edges.
11. Aspect-dependent defaults must not be frozen at first write.
12. Language list is de/en/auto here.
13. Breath only for transcript-made cuts.
14. Transcript must be inside the undo snapshot.
