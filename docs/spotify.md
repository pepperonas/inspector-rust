# Spotify Integration

Inspector works with the **Spotify desktop app on macOS**: it records what you
hear into a local history, shows the current track, exports the history and
controls playback. No Spotify account connection, no Web API, no OAuth —
everything stays on this Mac.

## Prerequisites

- macOS with the Spotify desktop app (`/Applications/Spotify.app` or
  `~/Applications/Spotify.app`).
- Inspector running — only plays while Inspector runs are recorded.
- For live position, the cover and the control commands: the **Automation**
  permission for Spotify (asked once, see *macOS permissions*). The history
  itself needs **no** permission.

## History

Inspector listens to `com.spotify.client.PlaybackStateChanged`, the
notification the Spotify app posts system-wide on every track or state change
(track, artist, album, track URI, duration, position, playing/paused/stopped).
There is no polling loop for it: a background thread (`ir-spotify`) only ticks
every 15 seconds without starting any process, to add up heard time, close long
pauses and notice that Spotify quit. If the Automation permission is already
granted, it additionally asks Spotify once a minute (AppleScript) to reconcile
and to read the cover URL — it never triggers the permission prompt itself.

### What makes one entry

A small state machine (`core/rust-lib/src/spotify/mod.rs`, `Tracker`) turns
the readings into plays:

| Situation | Result |
|---|---|
| A different track starts | the previous entry ends, a new one starts |
| Pause → resume of the same track | still **one** entry; the pause is not counted |
| Paused for 30 minutes | the entry ends at the moment it paused; resuming later starts a new one |
| Spotify stops or quits | the entry ends |
| Repeat-one: the same track jumps from its end back to the start | a new entry per loop (seeking back mid-track is not) |
| Ads (`spotify:ad:…`) | not history; they end the current entry |
| Played less than 5 seconds | dropped — clicking through a playlist is not listening |
| The Mac slept / the recorder stalled | a gap counts at most 60 s |
| Inspector quit while a track played | the entry is continued if the same track is still playing within 3 minutes; otherwise it is closed |

Heard time (`played`) is measured separately from the track length
(`duration`): only time in the *playing* state counts, and only while it can be
heard. Playback counts as silent when the Mac's output is muted or at 0 % (read
from CoreAudio; a mute on the real output behind boom counts too) or Spotify's
own volume is 0. Spotify's volume is only known through AppleScript, so it is
refreshed when Inspector queries Spotify (about once a minute, only with the
Automation permission); without that permission only the system volume decides.
An unknown reading counts as audible. A track played entirely in silence never
reaches the 5-second minimum and does not appear in the history.

### listened / partial / skipped

One rule, defined in `classify` (constants at the top of `spotify/mod.rs`):

- **listened** — at least half of the track, or at least 4 minutes
  (Last.fm's scrobble rule);
- **skipped** — under 30 seconds (and not listened);
- **partial** — everything in between.

Without a known track length only the absolute limits apply.

In the history panel each status has its own icon and a coloured left edge
(`components/PlayStatusIcon.tsx`):

| Status | Icon |
|---|---|
| listened | green ring, bold arrow running into an end stop |
| partial | orange tilde in a ring that fills to the heard share (tooltip: `angespielt · 42 %`); no ring fill without a known length |
| skipped | rose double chevron |
| playing | three green bars that bounce (still under reduced motion and the Off stage) |
| paused | two static slate pause bars — the entry is still open and closes after 30 minutes of pause |

The text label stays next to the time, and the icon carries an `aria-label`.

## Commands

All under `spotify` (type `spotify?` for the in-app help). Reads show while you
type; actions (controls, export) run on **Enter**, including repeated runs while the panel is focused. Custom values such as `volume 7` run as entered rather than accepting a preset. `stop` is an alias for `pause`.

Both German and English interval words are always offered, independent of the app language. Choosing another interval replaces the existing one; Tab cycles through intervals without appending them. Enter accepts a suggested interval, and another Enter runs the completed command. Add a space to continue with another filter or an export. If multiple intervals are typed manually, the last one wins.

| Command | What it does |
|---|---|
| `spotify` / `spotify now` | Current track, progress, controls, the last five plays |
| `spotify history` | The log, newest first, grouped by day (Heute, Gestern, …) |
| `spotify history heute` · `gestern` · `woche` · `monat` | Range filters (also `today`, `yesterday`, `week`, `month`) |
| `spotify history artist <name>` | Only that artist |
| `spotify history <text>` | Search track, artist and album (all words must match) |
| `spotify play` · `pause` · `stop` · `toggle` · `next` · `previous` | Control the app (`play` may launch Spotify) |
| `spotify volume <0-100>` | Spotify's own volume |
| `spotify seek <90 \| 1:30>` | Jump to a position |

With the panel focused (Enter): Space plays/pauses, ←/→ previous/next, ↑/↓
scroll the history, Esc leaves. The history loads 100 entries at a time
("Mehr laden").

### Enter, completion and repeated commands (v0.215.1)

A fully typed control runs on the first Enter. It also runs after another
Spotify command has already focused the panel: `spotify pause`, Enter,
`spotify play`, Enter, `spotify pause`, Enter does not require a list click.
Each Enter dispatches the current command once. Merely changing the text
does not execute a control or export.

| Input | Action on Enter | Accepted aliases |
|---|---|---|
| `spotify` / `spotify now` | Shows/focuses the current track | `jetzt` |
| `spotify play` | Starts/resumes playback | — |
| `spotify pause` | Pauses playback | `stop` |
| `spotify toggle` | Toggles play/pause | — |
| `spotify next` | Next track | `weiter`, `skip` |
| `spotify previous` | Previous track | `prev`, `back`, `zurück` |
| `spotify volume 7` | Sets Spotify volume to exactly 7% | `vol`, `lautstärke` |
| `spotify seek 1:30` | Seeks to 90 seconds | `spulen` |
| `spotify history` | Shows/focuses the local history | `verlauf`, `h` |
| `spotify history export csv` | Exports the current filter as CSV | Also `json`, `md`, `markdown`; no format defaults to JSON |

Volume accepts 0–100; presets are suggestions, so a typed `7` is not replaced
by the `75` suggestion when Enter is pressed. Seek accepts seconds, fractional
seconds using a dot or comma, `m:ss`, or `h:mm:ss`. Invalid values remain
validation errors. Tab can still accept a numeric preset explicitly.

An incomplete option, such as `spotify p`, keeps the selectable completion
rows. Enter accepts the selected option first; another Enter runs the
completed command. After `spotify history`, select a range, artist filter or
export in the same way. Names from the local artist history remain one value,
including names containing spaces.

### Switching history intervals

The following interval words are always offered, whether the app interface
is English or German:

| English | German | Window |
|---|---|---|
| `today` | `heute` | Since local midnight |
| `yesterday` | `gestern` | Previous local day, ending at today's midnight |
| `week` | `woche` | Today and the preceding six local days |
| `month` | `monat` | Today and the preceding 29 local days |

These bounds follow local calendar dates, including daylight-saving changes;
`month` means 30 days, rather than the current calendar month.

After entering or selecting `spotify history heute`, press Tab to switch to
another interval and Shift+Tab to go back. This remains available when the
interval was typed in full, clicked, or accepted with Enter. The query has
one interval at a time:

```text
spotify history heute
spotify history gestern
spotify history woche
```

Adding a space reopens filter/export suggestions. Choosing a different range
there replaces the earlier range too. It does not produce
`spotify history heute gestern gestern`. For manually typed or previously
saved commands containing several intervals, the last interval wins and the
other interval words do not become search terms.

Ranges combine with text, artist and export filters, for example:

```text
spotify history today
spotify history week artist Kraftwerk
spotify history gestern export md
spotify history month artist Parra for Cuva export csv
```

The artist name extends up to `export`; therefore place the interval before
`artist <name>`. With no range, `spotify history` shows all recorded plays.

### Implementation and verification

`App.tsx` keeps its Enter/list handler enabled when Spotify is focused.
`SpotifyPanel` consumes its own playback/history shortcuts and native button
Enter, leaving Enter from the search field available for command dispatch.
Its run-signal guard continues to prevent an action from replaying on remount.

`SPOTIFY_RANGES` in `lib/spotify.ts` supplies the parser and completion logic.
Range candidates replace earlier range tokens and complete without a trailing
space, so Enter can run them. `spotifyRangeCycle` reconstructs all eight
spellings for Tab switching. `showArgSuggestions` keeps valid controls
runnable even if their numeric value prefixes a preset; root aliases are
also recognised while walking the completion tree.

Regression tests cover controls and aliases through repeated keyboard Enter,
all three export formats, custom numeric values, English ranges with a
German interface, replacement of duplicate intervals, forward/backward Tab
cycling, and artist names that happen to match interval words. IPC calls are
mocked in these tests; they do not control a real Spotify session.

## Export

`spotify history export [json|csv|md]` (default JSON) — combinable with the
filters, e.g. `spotify history woche artist Kanye West export csv`. The
history panel has the same three buttons. Files go to `~/Downloads` as
`spotify-history-YYYY-MM-DD.<ext>`; an existing file is never overwritten
(`… 2.json`). Oldest entry first.

- **JSON** — `{ source, version, exported_at, count, plays: [...] }`, each play
  with `started_at`, `ended_at`, `track`, `artist`, `album`, `track_uri`,
  `duration_seconds`, `played_seconds`, `status`.
- **CSV** — header `started_at,ended_at,artist,track,album,track_uri,duration_seconds,played_seconds,status`
  (RFC 4180 quoting; a cell starting with `= + - @` gets a leading `'` so a
  spreadsheet does not run it as a formula).
- **Markdown** — one table per day.

Timestamps are ISO 8601 with the offset in force at that moment
(`2026-10-11T02:24:12+02:00`); durations are whole seconds.

## Data storage

- Table `spotify_plays` in Inspector's database `history.db`:
  `~/Library/Application Support/InspectorRust/history.db`.
- Plaintext (like the Shazam history); not part of any sync or telemetry.
- An entry is written while it is still playing (`open = 1`), so a crash loses
  at most one tick; damaged rows are repaired when read, never dropped.
- Settings → Spotify: turn recording off (ends the current entry), show covers,
  clear the history (the playing entry stays).
- The only network request of the feature is the optional cover image from
  Spotify's image server (`i.scdn.co`), off by default.

## macOS permissions

- **History** — none. Spotify's notification is public to every app.
- **Live position, cover, controls** — *Automation → Spotify*. The first
  `spotify` command you confirm with Enter triggers the macOS prompt
  ("Inspector Rust wants to control Spotify"). Inspector checks the state with
  `AEDeterminePermissionToAutomateTarget` *without* prompting, so the
  background recorder never asks on its own.
- Denied by mistake: System Settings → Privacy & Security → Automation →
  Inspector Rust → Spotify (the panel has a button), or reset with
  `tccutil reset AppleEvents io.celox.inspector-rust`.

## Limits

- Only the Spotify app on this Mac — not the phone, not the web player, not
  plays while Inspector isn't running.
- Without the Automation permission, a track that was already playing when
  Inspector started appears with the next track/state change.
