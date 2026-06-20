# Timesheet (time tracking)

An **opt-in, offline** time-tracking feature: a search-bar command toggles a
tracking *session*; in the background the app records app-usage intervals by
focus change (plus browser tab via a local extension and active Claude-Code
usage), auto-pauses on inactivity, and a second command opens a day-navigable,
editable timesheet view with **CSV** and **self-contained HTML** export.

> **Status — incremental delivery.** This is built in the delivery order below;
> each step is a green-gated commit. **Done so far:** Steps 1–4 — the data
> layer, the tracker core (focus loop + retroactive idle auto-pause, macOS), the
> IPC commands, and the `track on` / `track off` search-bar command with a
> status toast. Next: the Timesheet tab (step 5).

## Privacy & security (by design)

- **Opt-in, off by default.** Tracking never starts automatically; `track on`
  starts a session, `track off` ends it. Pause any time.
- **Encrypted at rest.** `window_title` and `url` are AES-256-GCM-encrypted
  using the app's existing key (OS keychain, `crypto.rs`) — the same path as
  clipboard history / snippets / notes. They are stored as the `"v1:<base64>"`
  string in **TEXT** columns (matching `entries.content_text`; no new crypto
  path). `app_name`, `host`, `category`, `project` and timestamps stay
  **plaintext** because aggregation/queries run on them.
- **No telemetry, no cloud, fully offline.** The only socket is a **loopback**
  WebSocket bound to `127.0.0.1` for the browser extension (token-authenticated)
  — never `0.0.0.0`, no outbound requests.
- **Denylist** (planned, Settings): for listed apps/domains only the app name +
  time are stored — no title, no URL.
- **Clear timesheet data** wipes everything (`track_db::clear_all`), with confirm.

## Data model (SQLite, in the one app DB)

Created idempotently in `db::open` via `tracking::db::init_schema`
(`CREATE TABLE IF NOT EXISTS`, the repo's migration convention):

- `track_sessions` — one tracking span (`status` = `active`/`paused`/`ended`).
- `track_events` — one focus/browser/claude interval (`source` =
  `focus`/`browser`/`claude`, `is_idle`, `started_at`/`ended_at`/`duration_s`;
  encrypted `window_title`/`url`; plaintext `host`/`category`/`project`).
- `track_claude_turns` — optional per-turn Claude token detail (1:n to an event).
- `track_categories` — user-maintained `app_name → category` mapping.

The persistence layer (`core/rust-lib/src/tracking/db.rs`) provides:
session lifecycle (`start_session`/`set_session_status`/`end_session`/
`active_session`), event lifecycle (`open_event`/`close_event` with
denormalised `duration_s`, `enrich_event` for browser tab metadata on the open
event), editing (`update_event`/`delete_event`/`merge_events`/`set_category`),
the range query (`events_in_range`, title/url decrypted), Claude turns, and
`clear_all`. All exercised by in-memory unit tests.

## Architecture (fixed decisions)

1. **Event-based, not sampling** — a focus change closes the open interval and
   opens a new one. No periodic snapshots.
2. **Pure analytics** — no billing/rates/clients; optional app→category mapping.
3. **Browser via extension, role split** — the desktop tracker is the *clock*
   (it decides from the frontmost app whether a browser is active); the
   extension is only a URL/title source (reports the active tab over the
   loopback WS). Per-tab duration comes from splitting the browser interval on
   tab change. No double-counting — the desktop stays the only time source.
4. **Claude-Code detection** — a `notify` watcher on `~/.claude/projects/**/*.jsonl`;
   appends mean active usage, aggregated per `cwd`, closed after a gap. Runs only
   while a session is active. Defensive JSONL parsing.
5. **Start/stop + idle auto-pause** — input inactivity beyond a threshold
   (default 300 s) retroactively closes the open interval as `idle`; input
   resumes a new interval. Idle is kept (visible/editable), not deleted, and not
   counted as active.

## Delivery order

1. **DB migration + schema + crypto + tests.** ✅ done
2. Tracker core: session state, focus loop, per-OS active window. ✅ done (macOS)
3. Idle detection + auto-pause. ✅ done (macOS)
4. IPC commands + search-bar `track on/off` + toast. ✅ done (footer LED/tray pending)
5. Timesheet tab (read-only: day navigation + charts).
6. Inline editing.
7. CSV + HTML export.
8. Claude watcher.
9. Browser bridge (loopback WS) + extension + options page.
10. Settings (idle, denylist, retention, bridge token) + docs + remaining OS modules.

## Export (planned)

- **CSV** — flat rows, UTF-8:
  `date,start,end,duration_min,app,category,project,host,title,source,idle`.
- **HTML** — a single self-contained file (CSS+JS+data inline, zero external
  requests), dark-themed, with charts (daily totals, app donut, day Gantt, top
  hosts, categories, Claude tokens). Footer: `© 2026 Martin Pfeffer | celox.io`.
