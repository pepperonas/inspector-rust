# Local audio/video transcription

Enter `transcribe` in the Inspector search bar to transcribe the audio or video
files selected in Finder. Like `md2pdf`, an explicit path takes precedence:

```text
transcribe
transcribe --language de
transcribe "~/Videos/Mein Interview.mp4" --language de
transcribe -l en ~/Downloads/meeting.m4a
transcribe ~/Downloads/meeting.mp4 --language=auto
```

The engine is the same as `meeting`: whisper.cpp with **large-v3-turbo** on the
Mac's GPU (Metal), in the `ir-transcribe` helper process. It runs on **Apple
Silicon Macs only** and transcribes **German and English**. `--language` / `-l`
takes `de`, `en` or `auto` (also `german`, `deutsch`, `english`). Without it
the language is **German**; `auto` detects German or English per segment. Place the option before or after the path. Quote paths containing
spaces; unquoted spaces are also preserved. Use `--` before a relative filename
that starts with a dash.

The speech model is downloaded once, from the `meeting` panel or Settings →
Live transcription (large-v3-turbo q5, ~574 MB). The model chosen there is used
here too.

Supported extensions: MP4, MP3, M4A, WAV, AAC, FLAC, OGG, OPUS, WebM, MOV, MKV,
AVI (case insensitive). Only supported files in a mixed Finder selection are
processed. Core Audio reads WAV, M4A, MP3, AAC, FLAC and the audio of MP4
directly; MOV, MKV, WebM, OGG, OPUS and AVI need **ffmpeg** (`brew install
ffmpeg`) — without it that file fails with a hint and the rest of the batch
still runs.

## Live dictation

```text
transcribe --start
transcribe --start --language de
transcribe --stop
```

`--start` records the microphone and shows the text in the preview while you
speak. As soon as the recording runs, the search bar reads `transcribe --stop`,
so the next Enter ends it — Enter stops a running dictation whatever the line
says. The **Stop recording** button does the same; the bin icon discards the
recording without copying anything.

How it works:

- The helper runs a session in `Mode::Dictation`: the microphone only, the same
  voice detection (Silero), segmentation and decoding as `meeting`. The open
  segment is shown greyed (the part two passes agree on stays, the rest may
  still change); a finished segment is kept.
- When you stop, what is still open is decoded, the kept text is copied to the
  clipboard and saved in the clipboard history. There is no second "accurate"
  pass any more — large-v3-turbo is already the accurate model.
- Esc or closing the popup does not stop the recording. A red **Dict** light
  in the footer shows it is still listening; type `transcribe` to see it or
  stop it. A dictation ends automatically after 60 minutes.
- Nothing leaves the machine and nothing is kept: the session writes into a
  private cache folder that is deleted at the end. (Song modes send a short
  audio fingerprint to Shazam — see below.)

A dictation, a song session, a file transcription and a `meeting` cannot run at
the same time — the second one reports "busy".

### Lyrics from a song: `transcribe --song`

`transcribe --song` (also `--lyrics`, `--system`) writes down what this Mac
plays instead of what the microphone hears. The helper takes the system audio
directly (the Core Audio process tap, ScreenCaptureKit on older systems) — no
speaker, room or microphone in between, and the music stays at full volume.
Panel: "Transcribe song" next to "Start recording".

Why a separate source: through the microphone, a song at full volume came back
as nothing at all (field log 2026-10-21). Taken digitally, Whisper reads sung
lyrics. Song sessions use 20-s segments (`Mode::Song`), because a sung line
needs more context than a spoken one.

### A song from another device: `transcribe --room`

`transcribe --room` (also `--raum`, or `--song --mic`) listens through the
microphone to a song playing elsewhere — a stereo, a phone, the radio. Panel:
"Song from another device". Two things run side by side:

- **Shazam recognises the song.** Every 8 s the app asks the helper for the
  last 10 s of audio (`Command::Window`) and sends a fingerprint (not the audio
  itself) until a match, at most 15 tries. When the lyrics catalogue (lrclib)
  has the song, its **exact lyrics** are copied at the stop instead of what
  Whisper heard, with a "Title – Artist" line on top. The match also lands in
  the Shazam history. `--song` (this Mac) does the same recognition.
- **Whisper writes along** for songs the catalogue lacks (`Mode::Room`,
  20-s segments).

Vocal separation (Demucs) before Whisper was tried in 0.225 and made it worse,
so it is not used. Through the microphone the system audio (`--song`) stays
more exact; use `--room` only when the song is not on this Mac.

### Turning the music down while dictating (opt-in)

With the setting `transcribe.duck_audio = true`, a dictation from the
microphone lowers the system output to 10 % and restores it afterwards (not if
you changed the volume meanwhile). Off by default: the music is never touched
unless you ask for it, and never in song mode.

## Results

The command checks the input and starts a background job. The overlay stays
open and the preview shows every file with its own progress bar, the percentage,
an estimate of the remaining time and, when done, the detected language and the
duration. Click a finished file to show its transcript in Finder.

Press Esc to close the view (or the whole overlay): the batch keeps running.
Type `transcribe` again while it runs to see the progress again — Enter does
not start a second run then. "Abbrechen" (click twice) stops the batch;
transcripts already written stay. When the batch ends while the view is closed,
a status message reports the result; macOS also shows a system notification.
A failed file does not stop the remaining batch.

The files are transcribed one after another, streamed: the audio is decoded,
resampled to 16 kHz mono, cut at pauses and each segment is decoded as it
comes — a long file never sits in memory. Progress moves after every segment.

Each transcript is UTF-8 plain text in the input folder, one line per segment:
`clip.mp4` becomes `clip.txt`. Existing files are never overwritten: subsequent
runs create `clip 2.txt`, `clip 3.txt`, etc. (gaps are reused). Output creation
is exclusive, including when runs finish at the same time; a failed write
removes its partial output. Repetition loops on silent or instrumental
passages are dropped (≥ 3 identical lines in a row), and the same filter as in
`meeting` removes Whisper's known silence phrases. No timestamps, no speaker
labels, no translation.

Measured on an M1 Pro (large-v3-turbo q5): a 15-s segment decodes in ~1 s; the
German test sentence (5 s) as WAV and as M4A came back word for word.

## Troubleshooting

- **No media selected:** select the file(s) in Finder before opening
  Inspector, or specify a path.
- **"The speech model is not downloaded yet":** open `meeting` and download it.
- **"This format needs ffmpeg":** install it (`brew install ffmpeg`); Homebrew's
  locations are found when Inspector is launched from Finder.
- **Other languages:** only German and English are supported; use `--language
  de`, `en` or `auto`.
- **Video without audio:** that file fails; other files in the selection still
  run.
- **"Live transcription needs a Mac with Apple Silicon":** Intel Macs, Windows
  and Linux have no transcription.

## Implementation

`core/rust-lib/src/transcribe.rs` (arguments, Finder selection, the per-file
job), `live_transcribe.rs` (dictation and song sessions), `transcribe_client.rs`
(the helper process). In the helper: `transcribe-core/src/file.rs` (decoding
via `ExtAudioFile`, ffmpeg fallback, segmentation, TXT naming) and
`pipeline.rs` (the live session). Commands and events are JSON lines between
app and helper (`transcribe-protocol`); paths never go through a shell.

Tests: `cargo test -p transcribe-core --lib file::` (segmentation, progress,
naming, Core Audio reading the fixture) and, with the model installed,
`cargo test --release -p transcribe-helper --test helper_protocol file_mode --
--ignored --nocapture` (WAV and an `afconvert`-made M4A end to end).

The Python worker (`openai-whisper`, `scripts/setup-transcribe.py`) used until
0.232 is gone; its virtual environment under
`~/Library/Application Support/InspectorRust/transcribe/venv` can be deleted.
