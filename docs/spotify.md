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
(`duration`): only time in the *playing* state counts.

### listened / partial / skipped

One rule, defined in `classify` (constants at the top of `spotify/mod.rs`):

- **listened** — at least half of the track, or at least 4 minutes
  (Last.fm's scrobble rule);
- **skipped** — under 30 seconds (and not listened);
- **partial** — everything in between.

Without a known track length only the absolute limits apply.

## Commands

All under `spotify` (type `spotify?` for the in-app help). Reads show while you
type; actions (controls, export) run on **Enter**.

| Command | What it does |
|---|---|
| `spotify` / `spotify now` | Current track, progress, controls, the last five plays |
| `spotify history` | The log, newest first, grouped by day (Heute, Gestern, …) |
| `spotify history heute` · `gestern` · `woche` · `monat` | Range filters (also `today`, `yesterday`, `week`, `month`) |
| `spotify history artist <name>` | Only that artist |
| `spotify history <text>` | Search track, artist and album (all words must match) |
| `spotify play` · `pause` · `toggle` · `next` · `previous` | Control the app (`play` may launch Spotify) |
| `spotify volume <0-100>` | Spotify's own volume |
| `spotify seek <90 \| 1:30>` | Jump to a position |

With the panel focused (Enter): Space plays/pauses, ←/→ previous/next, ↑/↓
scroll the history, Esc leaves. The history loads 100 entries at a time
("Mehr laden").

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
