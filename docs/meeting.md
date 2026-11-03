# meeting — live transcription of microphone and system audio

`meeting` (alias `mitschrift`, hotkey **Ctrl+Shift+Alt+L**) transcribes two
sources at once and keeps them apart:

- **MIC** — the microphone: you.
- **SYS** — everything this Mac plays: the other side of a call, a video.

Everything runs locally. Nothing leaves the Mac.

## Using it

Type `meeting`: the preview shows the sources, their levels and past
sessions. **Enter** starts, Enter again stops. The session runs in the
background helper, so the popup can close. While it records, the menu-bar
icon shows a red dot and the tray menu says "Stop transcription".

- Switch either source on or off in the panel, the tray or Settings.
- **Ctrl+Shift+Alt+K** shows or hides the ticker without stopping.
- **Reopening the popup** while a session runs loads `meeting` again, so the
  panel with stop, sources and language is right there.
- **Language while recording:** the DE / EN / Auto chips in the panel change
  it from the next sentence on, without stopping.
- **The ticker's own buttons** appear when the pointer is over it (otherwise
  it lets every click through to the app below): a grip to drag it anywhere
  (the place is remembered, per screen as fractions of the free space), −
  for a short pill, ⤢ for five wrapped lines, ⌖ to dock it at the screen
  edge again, × to switch it off.
- **Speakers:** MIC (this Mac's microphone, you) and SYS (what the Mac plays,
  the other side) are kept apart. Several people *within* SYS (or several in
  the room on MIC) are not told apart — there is no speaker recognition yet.
- The tray's "Open transcripts" opens the current session folder.

The first start needs the speech model (`large-v3-turbo`, 574 MB). The panel
offers the download, which is checked against the model's SHA-256.

## Files

Each session gets a folder `~/Documents/Transkripte/YYYY-MM-DD_HHMMSS/`
(Settings can change the root):

| File | Content |
|---|---|
| `transkript.txt` | One line per sentence: `[19:45:12] MIC: …` |
| `transkript.jsonl` | The same as JSON: start/end (ms since 1970), source, language, confidence, text |
| `session.json` | Start, end, model, sources, status `running` / `completed` / `aborted` |
| `audio/000001-mic.wav` | Optional: the raw audio of each sentence (16 kHz mono), kept for N days |

Every sentence is appended and synced to disk before the next one. If the
app or the helper is killed, at most the sentence being spoken is missing;
the next start marks the session `aborted`, trims a half-written last line
and says so in a notice. If the folder cannot be written (disk full, folder
gone), the session continues in `~/Library/Application Support/InspectorRust/Transkripte/`
with a warning; if that fails too, the session stops with an error — it
never records into nothing.

Previews (the grey text in the ticker) are never written.

## Recognition

- Whisper `large-v3-turbo` (5-bit) on the GPU via whisper.cpp. Smaller
  models can be chosen in Settings.
- Language: German by default (Settings → Live transcription). "Automatic"
  decides per sentence, but only between German and English (short sentences
  otherwise get mistaken for other languages); English can be fixed too.
- Speech detection: Silero VAD. A sentence is cut at a pause, at the latest
  after 15 s at its quietest moment.
- The ticker shows a preview every second; words two previews agree on stay
  white, the rest is grey until the sentence is final.

## Echo filter

Without headphones the microphone hears the speakers, and every sentence of
the other side would appear twice. A MIC sentence that overlaps a SYS
sentence in time and reads almost the same (similarity threshold in
Settings, default 75 %) is dropped. "Automatic" turns the filter off when
the output is headphones (Bluetooth, USB or the headphone jack).

## Permissions

macOS asks once for the **microphone** and for **system audio recording**
("audio capture"). If system audio cannot be tapped (macOS before 14.2), it
falls back to ScreenCaptureKit, which needs the screen recording permission.

## Limits

- Apple Silicon Macs only.
- Speech detection, not speaker recognition: several people on one source
  share its label.
- A change of the output device restarts the system-audio capture; about
  two seconds can be lost.

## Architecture

`transcribe-core` (capture, VAD, segmentation, LocalAgreement, echo gate,
writer, whisper.cpp) runs in the helper process `ir-transcribe`, which the
app talks to over JSON lines (`transcribe-protocol`). An abort inside
whisper.cpp ends the helper, never the app, and the model's memory is freed
when it exits.
