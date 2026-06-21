# Timesheet — analysis & presentation roadmap

Backlog of **planned** improvements to the Timesheet's analysis (Auswertung) and
presentation (Darstellung). Not yet built — captured here to implement later.
Ordered roughly by value-per-effort. Implementation notes reference the current
code so each can be picked up quickly.

Current building blocks to reuse:
- Backend: `tracking/mod.rs` — `aggregate_day` (per-day), `range_report`
  (multi-day, reuses `aggregate_day`), `union_seconds`, `DayReport` /
  `RangeReport` / `AppBreakdown` / `Bucket`. IPC in `commands.rs`
  (`track_get_day` / `track_get_range`), registered in `lib.rs`.
- Frontend: `components/TimesheetPanel.tsx` (day), `components/TimesheetWeek.tsx`
  (week), `lib/timesheet.ts` (pure helpers: `formatDuration`, `colorMap`,
  `paletteColor`, `donutSegments`/`donutSegmentPath`, `weekBounds`, …).

---

## Analysis (more insight)

### 1. Activity heatmap (hours × days) — highest insight/pixel
A week grid: rows = Mon–Sun, columns = 0–23h, cell intensity = active minutes in
that hour. Reveals *when* you're productive.
- **Backend:** extend `range_report` (or a new `heatmap(from,to)`) to return a
  `Vec<DayHours>` where each day has `hours: [i64; 24]` of active seconds. Bucket
  each active interval into hour slots (clip per hour). Pure + unit-testable.
- **Frontend:** a CSS-grid of cells in `TimesheetWeek`, opacity/color ∝
  value/max; tooltip per cell. Reuse the accent color.

### 2. Comparison vs. previous period
Δ values: "today vs. yesterday", "this week vs. last week" — e.g.
"+42 min · productivity +6 %".
- **Backend:** none needed — fetch the previous day/week report too and diff in
  the frontend. (Or add a `compare` helper.)
- **Frontend:** small Δ chips next to the totals (green/red, ▲/▼).

### 3. Daily focus goal / target
Set a target (e.g. 6 h focus) → progress ring + "1 h 12 m to go".
- **Backend:** store `track.daily_goal_minutes` in settings (extend
  `TimesheetConfig` + Settings UI). 
- **Frontend:** a progress ring (reuse `donutSegmentPath`) in the day totals; edit
  the goal in Settings → Timesheet.

### 4. Drill-down by click
Click a category/app → filter the day/week to just that one.
- **Frontend only:** a `filter: {kind, key} | null` state; when set, filter
  `report.events` / breakdown buckets before rendering; a removable filter chip.

### 5. Focus quality metrics
Longest uninterrupted session, number of context switches, a fragmentation
score (e.g. active_seconds / number_of_active_intervals).
- **Backend:** compute in `aggregate_day` from the active intervals (already
  collected as `active_iv` before the union); return `longest_focus_s`,
  `switch_count`. 
- **Frontend:** a small "Focus" stat card.

### 6. Event search / filter in the Timeline
Search box filtering events by app / title / host.
- **Frontend only:** a text input above the event list; filter `report.events`
  case-insensitively. (Titles are decrypted server-side already in the report.)

---

## Presentation (clearer / nicer)

### 7. Consistent category colors everywhere (+ category donut)
A stable category→color map used across all charts + as chips, so the same
category is always the same color. Add a category **donut** (today it's bars).
- **Frontend:** a `categoryColorMap` derived from the day/week's categories
  (stable order → `paletteColor`); thread it into the by-category bars, the
  timeline, and a new donut (reuse `Donut` from `TimesheetPanel`). Consider
  letting the user pin a color per category (settings map) later.

### 8. Stacked weekly day-bars by category
In the week view, split each day's bar by category (not just active/idle) — see
the week's composition at a glance.
- **Backend:** `range_report.days` needs per-day per-category seconds. Extend
  `DaySummary` with `by_category: Vec<Bucket>` (cheap — `aggregate_day` already
  has it per day).
- **Frontend:** render each day bar as stacked segments using the category color
  map from #7.

### 9. 24h timeline colored by category (App ↔ Category toggle)
The day timeline is app-colored; add a toggle to color by category instead.
- **Frontend only:** the timeline already maps events; switch the color source to
  the category color map (#7) when toggled.

### 10. Insight summary sentence
A one-line readout above the charts: "Today: 4 h 12 m active, 78 % productive,
top: Code (2 h)."
- **Frontend only:** compose from the existing `report` fields.

### 11. Chart polish
Hover tooltips with exact values; soft enter animations; sticky totals header
while scrolling; optional decimal hours (2.5 h) via a format toggle.
- **Frontend only:** small additions to the bar/donut components + `formatDuration`
  variant (`formatHours`).

---

## Suggested first batch (best value, self-contained)
1 (heatmap) · 7 (category colors + donut) · 8 (stacked weekly bars) ·
2 (comparison) · 3 (daily goal). The rest are mostly frontend-only polish that
can follow incrementally.

## Dependencies / notes
- #7's category color map is a prerequisite for the nicest versions of #8 and #9.
- Keep totals **union-based** (`union_seconds`) for any new total so overlaps
  never inflate — see `aggregate_day`.
- Per-OS / privacy model unchanged; these are pure aggregation + UI.
