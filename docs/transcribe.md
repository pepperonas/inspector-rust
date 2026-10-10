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

The command checks the input, language and dependencies before starting a
background worker. Inspector remains usable while Whisper processes the batch.
A status notification reports the written filename or any failures; macOS also
shows a system notification. A failed file does not stop the remaining batch.

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
