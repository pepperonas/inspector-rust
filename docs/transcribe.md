# Local audio/video transcription

Enter `transcribe` in the Inspector search bar to transcribe the MP4 or other
media files selected in Finder. Like `md2pdf`, an explicit path takes precedence:

```text
transcribe
transcribe --language de
transcribe "~/Videos/Mein Interview.mp4" --language de
transcribe -l en ~/Downloads/meeting.mp4
transcribe ~/Downloads/meeting.mp4 --language=auto
```

`--language` / `-l` accepts Whisper language codes (`de`, `en`, `fr`, …) and
English language names (`german`, `english`, …). Without it, or with `auto`, the
language is detected separately for each input. Place the option before or after
the path. Quote paths containing spaces; unquoted spaces are also preserved.
Use `--` before a relative filename that starts with a dash.

Supported extensions: MP4, MP3, M4A, WAV, AAC, FLAC, OGG, OPUS, WebM, MOV, MKV,
AVI (case insensitive). Only supported files in a mixed Finder selection are
processed. Finder selection requires macOS; Windows and Linux take a path.

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

- The microphone is captured natively (the same path as `bpm` / `iris`), never
  through the web view, so other apps' playback is not interrupted.
- The stream is cut into back-to-back pieces at pauses (≥ 0.6 s once a piece
  is 3 s long, any 1.5 s pause after sound), or after 12 s at the quietest
  moment. Nothing is thrown away except true silence — an energy rule cannot
  tell a voice from music, so Whisper decides what is speech. Each finished
  piece is transcribed with the fast `base` model and kept; the one still
  being spoken is shown greyed and updated about every 1.5 s.
- Background music works: Whisper's own confidence drops what it invents on
  music (measured: speech scores −0.2…−0.5, inventions −3…−6, cut at −1).
  Tested with a song under the dictation — complete up to music as loud as
  the voice; louder than the voice a word or two can go wrong.
- After the stop, the accurate `small` model runs once over the whole
  recording (a few seconds) and replaces the live text. That final text is
  copied to the clipboard and saved in the clipboard history. If the accurate
  pass fails, the live text is copied instead — dictated words are never lost.
- Esc or closing the popup does not stop the recording. A red **Dict** light
  in the footer shows it is still listening; type `transcribe` to see it or
  stop it. A dictation ends automatically after 60 minutes.
- Nothing leaves the machine. The audio is kept in memory and in a private
  cache folder only while the session runs.

The first dictation downloads the `base` model (~140 MB). Override the models
with `IR_WHISPER_LIVE_MODEL` (live, default `base`) and `IR_WHISPER_MODEL`
(final pass, default `small`). A dictation and a file transcription cannot run
at the same time.

Measured on an M1 Pro: a 3-second sentence takes 0.6–0.9 s with `base`; the
final pass over 6.6 s of speech took 8 s with `small`.

### Music playing on this Mac

The microphone cannot separate a voice from loud music coming out of the same
Mac's speakers — Whisper then hears only the music and returns nothing. While a
dictation records, the system output volume is lowered to 10 % and restored
when the recording stops (or the app quits). If you change the volume during
the recording, your setting is kept. Turn it off with the setting
`transcribe.duck_audio = false`. Music from another device (a stereo) is not
affected — use headphones or turn it down by hand.

## Setup

Install Python 3.10 or newer and ffmpeg, then run from the repository:

```bash
# macOS
brew install python ffmpeg
python3 scripts/setup-transcribe.py

# Linux (Debian/Ubuntu)
sudo apt install python3 python3-venv ffmpeg
python3 scripts/setup-transcribe.py
```

On Windows, install Python and ffmpeg (available on PATH), then run
`py scripts/setup-transcribe.py`. The script installs `openai-whisper` and
`truststore` (OS certificates for model downloads), with their dependencies,
in an isolated environment below your local application data folder:

- macOS: `~/Library/Application Support/InspectorRust/transcribe/venv`
- Linux: `${XDG_DATA_HOME:-~/.local/share}/InspectorRust/transcribe/venv`
- Windows: `%LOCALAPPDATA%\InspectorRust\transcribe\venv`

Alternatively, set `IR_WHISPER_PYTHON` to the Python executable in an existing
environment containing `openai-whisper`, then start Inspector from that
environment. Without a dedicated environment, Inspector tries system Python.
GUI launches also find ffmpeg in the usual Homebrew locations.

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

Whisper reports progress in 30-second windows, so a short clip jumps straight
from "Audio wird gelesen…" to done, and language detection happens before the
first step.

Each transcript is UTF-8 plain text in the input folder: `clip.mp4` becomes
`clip.txt`. Existing files are never overwritten: subsequent runs create
`clip 2.txt`, `clip 3.txt`, etc. Output creation is exclusive, including when
multiple runs finish at the same time. Failed writes remove their partial output.

Whisper's multilingual `small` model runs locally on CPU, loading once per batch.
Its weights are downloaded on first use, requiring internet and disk space;
afterwards it uses the cached model. Media is not uploaded. For development,
`IR_WHISPER_MODEL` can select another Whisper model (e.g. `tiny` for a smoke test).
Model names ending in `.en` support English only, so use a multilingual model
for other languages. This command transcribes without translation. It does not
produce timestamps or identify speakers. Transcription accuracy depends on the
recording; a video must contain an audio track.

## Troubleshooting

- **No media selected:** select the file(s) in Finder before opening Inspector,
  or specify a path. On Windows/Linux an explicit path is required.
- **Whisper or Python unavailable:** run the setup script above, or point
  `IR_WHISPER_PYTHON` at an environment with `openai-whisper` installed. The
  dedicated environment is detected automatically by the installed app.
- **ffmpeg unavailable:** install it and ensure it is on PATH. Homebrew's standard
  locations are also detected when Inspector is launched from Finder.
- **Unknown language:** use a supported Whisper code such as `de` or `en`, an
  English name such as `german`, or `auto`. `-l` and `--language` are equivalent.
- **Download fails:** the first run needs internet access. The setup includes
  `truststore` so model downloads use the operating system's certificates.
- **Video without audio:** that file fails with the decoder's error; other files
  in the selection still run.

## Validation

The macOS worker was tested with a German MP4, both with `--language de` and with
automatic detection. Existing TXT files stayed intact; new outputs used ` 2`
and ` 3` suffixes. A batch containing a video without audio continued to the next
file. Offline worker tests: `python3 -B scripts/test-transcribe.py`; Rust parser,
selection and summary tests live in `transcribe.rs`. Windows/Linux runtime use
has not been verified.

Implementation: shared Rust `transcribe.rs`, embedded Python worker
`assets/transcribe/runner.py`, IPC `transcribe_run`. Paths/language travel as
JSON over stdin, never through shell interpolation.

Whisper API reference: [official implementation](https://github.com/openai/whisper/blob/main/whisper/transcribe.py).
