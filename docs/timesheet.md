# Timesheet (time tracking)

An **opt-in, offline** time-tracking feature: a search-bar command toggles a
tracking *session*; in the background the app records app-usage intervals by
focus change (plus browser tab via a local extension and active Claude-Code
usage), auto-pauses on inactivity, and a second command opens a day-navigable,
editable timesheet view with **CSV** and **self-contained HTML** export.

> **Status — incremental delivery.** This is built in the delivery order below;
> each step is a green-gated commit. **Done so far:** Steps 1–9 — data layer, tracker core, IPC, the Timesheet tab (charts, inline editing, CSV/HTML export), the Claude watcher, and the **browser bridge** (loopback WS + MV3 extension in `extension/`). Next: Settings + remaining OS modules (step 10).

## Consolidated slots (v0.90.0)

Recording produces one event per focus change. That is the right granularity to
*measure* with, and the wrong one to *book* with: a normal day reaches several
hundred fragments of a few seconds each (463 of 1–30 s in the case that
prompted this), and nobody transfers that into a timesheet by hand.

`tracking/slots.rs` derives the few contiguous blocks a person would actually
book. It is **non-destructive** — slots are recomputed on demand, every slot
carries the ids of the events it came from, and the raw timeline stays exactly
as recorded. (The older "Clean up" button is the opposite: it DELETEs idle spans
and sub-15 s rows and cannot be undone. It remains, but is no longer the way to
get a readable day.)

Two pure, unit-tested steps:

1. **Project inference** — most events carry no project, so it comes from the
   strongest signal available: an explicit tag, the Claude working directory,
   project-name tokens in the window title/URL/host, and finally temporal
   neighbourhood (untagged time next to a project block inherits it; an idle
   span breaks the chain, so a project never bleeds across a break). Which
   signal named a slot is shown in the UI — a guess should look like one.
2. **Consolidation** — contiguous runs of the same project, bridging short
   gaps, **absorbing foreign interruptions shorter than `noise_s`** (this is the
   lever that actually removes the fragmentation: a 20-second glance at Slack
   must not split a two-hour block into three), split by real breaks, then
   snapped to the quarter-hour grid.

**Rounding may not inflate the day.** Edges snap to the *nearest* quarter and
the minimum-length gate runs on the *measured* duration, before snapping. Both
were verified the expensive way in the sibling project bcsbook: rounding outward
turned a measured 7-hour day into 7.5 booked hours, and filtering after snapping
let a 20-minute stretch pass a 30-minute floor as a 45-minute block. Timesheets
feed billing, so an upward bias is the costly direction to be wrong in.

### Handover to bcsbook

`tracking/bcsbook.rs` maps slots onto the row shape of **bcsbook** (the tool
that books hours into Projektron BCS) via a `project = shortcut` table and posts
them to its local API for review and booking.

bcsbook's `POST /api/suggestions/save` **replaces the whole day**, so pushing
blindly would silently wipe whatever else that day holds — rows derived from git
commits, from bcsbook's own presence tracking, and any manual correction. The
push therefore reads the day first and merges: by default only slots that do not
collide with an existing entry are added, and the result reports what was
skipped and which projects still lack a shortcut. Replacing the day is possible
but has to be asked for explicitly.

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
   **⚠️ Socket gotcha (the v0.84.239 battery-drain bug):** the bridge's
   listener is non-blocking (for its stop-flag accept loop), and on
   **macOS/BSD an `accept()`ed socket inherits O_NONBLOCK** (Linux clears
   it) — so the per-connection handler MUST `set_nonblocking(false)` on the
   accepted stream before relying on its read timeout. Without that reset,
   `ws.read()` returned `WouldBlock` instantly, the timeout never engaged,
   and the connection loop **spun a full CPU core for as long as the
   extension was connected** (~4.6 CPU-hours in one morning, measured live).
   A regression test (`accepted_stream_blocks_after_clearing_nonblocking`)
   pins the blocking behaviour; a 10 ms sleep in the `WouldBlock` arm is the
   belt-and-braces cap.
4. **Claude-Code detection** — a `notify` watcher on `~/.claude/projects/**/*.jsonl`;
   appends mean active usage, aggregated per `cwd`, closed after a gap. Runs only
   while a session is active. Defensive JSONL parsing.
5. **Start/stop + idle auto-pause** — input inactivity beyond a threshold
   (default 300 s) retroactively closes the open interval as `idle`; input
   resumes a new interval. Idle is kept (visible/editable), not deleted, and not
   counted as active.
6. **Survives restarts** — the open event is heartbeated (`db::touch_event`)
   every tick, so a crash/quit/update leaves it ended at the last-alive moment
   (no phantom offline time). At startup `resume_if_active` re-arms the loop +
   watcher + bridge on the **same** session if it wasn't cleanly ended, so
   recording continues; the offline gap is simply not recorded.

## Delivery order

1. **DB migration + schema + crypto + tests.** ✅ done
2. Tracker core: session state, focus loop, per-OS active window. ✅ done (macOS)
3. Idle detection + auto-pause. ✅ done (macOS)
4. IPC commands + search-bar `track on/off` + toast + footer REC LED. ✅ done
5. Timesheet tab (read-only: day navigation + charts). ✅ done
6. Inline editing. ✅ done
7. CSV + HTML export. ✅ done
8. Claude watcher. ✅ done (macOS/all where ~/.claude/projects exists)
9. Browser bridge (loopback WS) + extension + options page. ✅ done
10. Settings (idle, retention, denylist, Claude toggle, bridge token) + docs +
    Windows/Linux OS modules. ✅ done

**Status: feature-complete.** macOS is verified end-to-end. Windows
(`GetForegroundWindow`/`QueryFullProcessImageNameW`/`GetLastInputInfo`) and Linux
(X11 `xdotool` + `xprintidle`) active-window/idle modules are compile-validated
but **runtime-unverified** (the repo's accepted line for non-macOS code); Linux
under Wayland degrades cleanly to "no data". The `track` command is macOS-gated
in the UI until the other OSes are runtime-verified.

## Settings (Settings → Timesheet, macOS)

- **Idle threshold (seconds)** — `track.idle_seconds` (default 300).
- **Retention (days)** — `track.retention_days` (default 0 = keep forever);
  applied on the next `track on` (`db::prune_before`).
- **Claude Code usage** — `track.claude_watcher` (default on).
- **Privacy denylist** — `track.denylist` (comma/newline app names or hosts);
  matches store only app + time (title/url/host stripped, no URL-split).
- **Browser extension** — the loopback bridge port + token (in the Timesheet
  tab's "Browser extension" disclosure), with copy + regenerate.

## Projects (client billing)

Assign tracked time to **projects** and export a client-facing report:
- **Assign** — in the day view, **drag a window on the day-timeline (Gantt)**; a
  popover shows the range + matching entries → pick a project (autocomplete) →
  every active event the window overlaps is tagged with it (whole events; idle +
  Claude events are skipped). The day timeline can be **coloured by project**
  (App ↔ Category ↔ Project toggle, with a legend); a **By project** section
  lists the day's entries grouped per project (expandable), including a
  "(no project)" group for still-unassigned active time. Projects also drive the
  day/week **By project** breakdowns.
- **Export** — the Timesheet tab's **Project export** footer: pick a free date
  range, a **project** (a single client so others aren't exposed, or *All
  projects* = default), and a **detail level**, then export **HTML** or **CSV**:
  - **Full** (default) — every entry (date · time · duration · activity).
    CSV `project,date,start,end,duration_min,app,activity`.
  - **Per day** — one total per project per day (no app/title).
    CSV `project,date,duration_min`.
  - **Summary** — one total per project. CSV `project,duration_min`.
  Billable = active, non-Claude, project-tagged events (so Claude time can't
  double-bill terminal focus). A single-client export filename includes the
  project slug. IPC `track_set_project` / `track_export_projects(format, from,
  to, project, detail)`.

## Export (full history)

- **CSV** — flat rows, UTF-8:
  `date,start,end,duration_min,app,category,project,host,title,source,idle`.
- **HTML** — a single self-contained file (CSS + inline-SVG charts, zero external
  requests), dark-themed, with daily totals, app donut, category + top-host bars,
  a **By app (detailed)** section (one collapsible `<details>` per app — native
  expand, no JS — browsers list visited hosts, other apps list window titles,
  each with count + time), a Claude-Code section, and the event table. Footer:
  `© 2026 Martin Pfeffer | celox.io`.

The tab has a **Day ↔ Week** toggle. **Week** shows the Mon–Sun week: per-day
active/idle bars (click a day to open it), the week's category/app/project
breakdowns, and a productive-vs-idle ratio (`track_get_range` → `range_report`).
The **Day** view shows totals (Active / Idle / **Productive %**), charts, a
**By project** breakdown, and the events area with a **By app ↔ Timeline** toggle:
- **By app** — every app grouped with its total usage; click an app (e.g. Google
  Chrome) to expand its detail/history (visited hosts for browsers, window titles
  for other apps, each with time + count). Each app's panel has a **category
  assign** field (datalist autocomplete) that sets the category on all its events
  + saves an app→category rule (new events auto-categorize; manage rules in
  Settings → Timesheet).
- **Timeline** — the editable chronological event list: inline edit (Enter saves,
  Esc cancels) · merge · delete · category · idle, plus **+ Add entry** (log
  forgotten time by hand) and **Clean up** (delete the day's idle spans + sub-15s
  fragments).
