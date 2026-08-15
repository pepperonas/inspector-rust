//! Consolidation of raw tracking events into bookable **project slots**.
//!
//! The tracker records one event per focus change, which is the right thing for
//! measurement but useless as a timesheet: alt-tabbing between an editor, a
//! terminal and a browser for three minutes produces a dozen rows of 1–30 s.
//! A real workday reaches several hundred fragments, and nobody books that.
//!
//! This module derives, from those fragments, the few contiguous blocks a human
//! would actually write on a timesheet. It is **non-destructive**: nothing is
//! deleted, slots are recomputed on demand, and every slot keeps the ids of the
//! events it came from so a booking can be traced back to the raw measurement.
//!
//! Two steps, both pure and unit-tested:
//!
//! 1. **Project inference** — the grouping key. Most events carry no project,
//!    so it is inferred from the strongest available signal: an explicit tag,
//!    the Claude working directory, project-name tokens in the window title /
//!    URL, and finally temporal neighbourhood (untagged time between two blocks
//!    of the same project belongs to that project).
//! 2. **Slot building** — contiguous runs of the same key, bridging short gaps,
//!    absorbing short foreign interruptions, split by real breaks, then snapped
//!    to the quarter-hour grid the target system books in.
//!
//! Rounding is deliberately to the *nearest* quarter and the minimum-length
//! gate runs on the *measured* duration, before snapping. Rounding outward and
//! filtering afterwards both inflate the day — a 7-hour day was offered as 7.5,
//! and a 20-minute stretch survived a 30-minute floor as a 45-minute block.

use super::db::TrackEvent;
use serde::{Deserialize, Serialize};

/// Tunables for the consolidation. Defaults are chosen for a normal knowledge-
/// work day; every one of them is exposed in the UI because the right value
/// depends on how someone works.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SlotParams {
    /// Two runs of the same project separated by at most this are one slot.
    pub bridge_gap_s: i64,
    /// A foreign event shorter than this does not break a slot — it is absorbed
    /// into the surrounding work. This is the main lever against fragmentation:
    /// a 20-second glance at Slack must not split a two-hour block in three.
    pub noise_s: i64,
    /// An idle span at least this long is a real break and always splits.
    pub min_break_s: i64,
    /// Slots shorter than this are dropped (measured duration, before snapping).
    pub min_slot_s: i64,
    /// Snap grid in minutes for the final slot bounds; 0 disables snapping.
    pub grid_min: i64,
    /// Untagged time adjacent to a project block inherits that project when the
    /// gap is at most this. Set to 0 to disable neighbourhood inference.
    pub neighbour_gap_s: i64,
}

impl Default for SlotParams {
    fn default() -> Self {
        Self {
            bridge_gap_s: 300,
            noise_s: 90,
            min_break_s: 300,
            min_slot_s: 900,
            grid_min: 15,
            neighbour_gap_s: 600,
        }
    }
}

/// How a slot's project was determined — shown in the UI so the user can see
/// which rows deserve a second look before booking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// The event already carried a project tag (manual assignment).
    Tagged,
    /// Claude-Code working directory.
    Claude,
    /// A project name appeared in the window title, URL or host.
    Title,
    /// Inherited from neighbouring events of the same project.
    Neighbour,
    /// No project could be determined — grouped by application instead.
    Unassigned,
}

impl Origin {
    /// Ranking used when several events with different origins land in one
    /// slot: the strongest signal names the slot.
    fn rank(self) -> u8 {
        match self {
            Origin::Tagged => 4,
            Origin::Claude => 3,
            Origin::Title => 2,
            Origin::Neighbour => 1,
            Origin::Unassigned => 0,
        }
    }
}

/// One event after project inference.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub app: String,
    pub detail: Option<String>,
    pub project: Option<String>,
    pub origin: Origin,
    pub is_idle: bool,
    pub is_claude: bool,
}

impl Resolved {
    fn seconds(&self) -> i64 {
        ((self.end_ms - self.start_ms).max(0)) / 1000
    }
    /// The grouping key: the project when known, otherwise the app, so
    /// unassigned time still consolidates instead of staying fragmented.
    fn key(&self) -> String {
        match &self.project {
            Some(p) if !p.is_empty() => format!("p:{p}"),
            _ => format!("a:{}", self.app),
        }
    }
}

/// A contributing application inside a slot, with its share of the time.
#[derive(Debug, Clone, Serialize)]
pub struct SlotApp {
    pub app: String,
    pub seconds: i64,
}

/// One consolidated, bookable block.
#[derive(Debug, Clone, Serialize)]
pub struct Slot {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Inferred project, `None` when the slot is app-grouped only.
    pub project: Option<String>,
    /// Human label — the project, or the dominant app when unassigned.
    pub label: String,
    /// Suggested booking description, built from the most-used detail strings.
    pub description: String,
    pub origin: Origin,
    /// Contributing apps, longest first.
    pub apps: Vec<SlotApp>,
    /// Raw events this slot was built from — the audit trail.
    pub event_ids: Vec<i64>,
    /// Actually measured active seconds (excludes bridged gaps).
    pub active_s: i64,
    /// Wall-clock seconds after snapping (what gets booked).
    pub span_s: i64,
    /// Share of `span_s` explained by the dominant key, 0…1. Low values mean a
    /// mixed block that is worth reviewing.
    pub confidence: f64,
}

// ── Project inference ────────────────────────────────────────────────────────

/// Lower-case a string and keep only alphanumerics, so `Kiez-Finder`,
/// `kiez_finder` and `kiezFinder` all reduce to the same token.
pub fn normalize_token(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Split a free-text string (window title, URL, host) into comparable tokens.
/// Very short tokens are dropped — matching on "de" or "io" would attach random
/// browser tabs to a project.
pub fn tokens_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(normalize_token)
        .filter(|t| t.len() >= 3)
        .collect()
}

/// Find a known project whose name appears in the given text. The longest
/// project name wins, so `inspector-rust` beats a project merely called
/// `inspector` when both are configured.
pub fn match_project_in_text(text: &str, known: &[String]) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let toks = tokens_of(text);
    if toks.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &String)> = None;
    for p in known {
        let pt: Vec<String> = tokens_of(p);
        if pt.is_empty() {
            continue;
        }
        // Every token of the project name must occur in the text — a
        // single-token project matches a single token, a two-word project
        // needs both, which keeps "kiez finder" from matching a stray "kiez".
        if pt.iter().all(|t| toks.iter().any(|x| x == t)) {
            let weight: usize = pt.iter().map(|t| t.len()).sum();
            if best.is_none_or(|(w, _)| weight > w) {
                best = Some((weight, p));
            }
        }
    }
    best.map(|(_, p)| p.clone())
}

/// The project names worth matching against: everything already tagged or
/// reported by Claude in this range, plus any extra names the caller supplies
/// (the configured project→shortcut mapping).
pub fn known_projects(events: &[TrackEvent], extra: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: &str| {
        let p = p.trim();
        if !p.is_empty() && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(p)) {
            out.push(p.to_string());
        }
    };
    for e in events {
        if let Some(p) = &e.project {
            push(p);
        }
    }
    for p in extra {
        push(p);
    }
    out
}

/// Resolve a project for every event, in order of signal strength.
///
/// The neighbourhood pass runs last and only fills events that no stronger
/// signal claimed: untagged time sandwiched between two blocks of the same
/// project (or directly adjacent to one) is that project's. This is what turns
/// "terminal, browser, terminal" around a Claude session into one block instead
/// of three.
pub fn infer_projects(events: &[TrackEvent], known: &[String], params: &SlotParams) -> Vec<Resolved> {
    let mut out: Vec<Resolved> = events
        .iter()
        .filter_map(|e| {
            let end = e.ended_at?;
            if end <= e.started_at {
                return None;
            }
            let is_claude = e.source == "claude";
            let tagged = e.project.as_deref().filter(|p| !p.is_empty()).map(str::to_string);
            let (project, origin) = if let Some(p) = tagged {
                let o = if is_claude { Origin::Claude } else { Origin::Tagged };
                (Some(p), o)
            } else {
                // Title / URL / host tokens.
                let mut hay = String::new();
                if let Some(t) = &e.window_title {
                    hay.push_str(t);
                    hay.push(' ');
                }
                if let Some(u) = &e.url {
                    hay.push_str(u);
                    hay.push(' ');
                }
                if let Some(h) = &e.host {
                    hay.push_str(h);
                }
                match match_project_in_text(&hay, known) {
                    Some(p) => (Some(p), Origin::Title),
                    None => (None, Origin::Unassigned),
                }
            };
            let detail = e
                .window_title
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| e.host.clone().filter(|s| !s.is_empty()));
            Some(Resolved {
                id: e.id,
                start_ms: e.started_at,
                end_ms: end,
                app: e.app_name.clone(),
                detail,
                project,
                origin,
                is_idle: e.is_idle,
                is_claude,
            })
        })
        .collect();
    out.sort_by_key(|r| r.start_ms);

    if params.neighbour_gap_s > 0 {
        fill_from_neighbours(&mut out, params.neighbour_gap_s * 1000);
    }
    out
}

/// Give untagged, non-idle events the project of a close neighbour. A gap on
/// both sides with the *same* project fills unconditionally; a one-sided
/// neighbour fills too, which covers the start and end of a work block.
fn fill_from_neighbours(events: &mut [Resolved], gap_ms: i64) {
    let n = events.len();
    // Nearest tagged project before / after each index, within the gap.
    let mut before: Vec<Option<String>> = vec![None; n];
    let mut last: Option<(usize, String)> = None;
    for i in 0..n {
        if events[i].is_idle {
            last = None; // a break interrupts the chain of thought, and the fill
            continue;
        }
        if let Some((li, p)) = &last {
            if events[i].start_ms - events[*li].end_ms <= gap_ms {
                before[i] = Some(p.clone());
            }
        }
        if let Some(p) = &events[i].project {
            last = Some((i, p.clone()));
        }
    }
    let mut after: Vec<Option<String>> = vec![None; n];
    let mut next: Option<(usize, String)> = None;
    for i in (0..n).rev() {
        if events[i].is_idle {
            next = None;
            continue;
        }
        if let Some((ni, p)) = &next {
            if events[*ni].start_ms - events[i].end_ms <= gap_ms {
                after[i] = Some(p.clone());
            }
        }
        if let Some(p) = &events[i].project {
            next = Some((i, p.clone()));
        }
    }
    for i in 0..n {
        if events[i].project.is_some() || events[i].is_idle {
            continue;
        }
        let pick = match (&before[i], &after[i]) {
            // Same project on both sides — the strongest case.
            (Some(b), Some(a)) if b == a => Some(b.clone()),
            // Different projects around it: prefer the one it started next to.
            (Some(b), Some(_)) => Some(b.clone()),
            (Some(b), None) => Some(b.clone()),
            (None, Some(a)) => Some(a.clone()),
            (None, None) => None,
        };
        if let Some(p) = pick {
            events[i].project = Some(p);
            events[i].origin = Origin::Neighbour;
        }
    }
}

// ── Slot building ────────────────────────────────────────────────────────────

/// Consolidate resolved events into slots. Pure — unit-tested.
pub fn build_slots(events: &[Resolved], params: &SlotParams) -> Vec<Slot> {
    // Working accumulation for one slot.
    struct Acc {
        start_ms: i64,
        end_ms: i64,
        per_key: std::collections::HashMap<String, i64>,
        per_app: std::collections::HashMap<String, i64>,
        per_detail: std::collections::HashMap<String, i64>,
        project_by_key: std::collections::HashMap<String, (Option<String>, Origin)>,
        ids: Vec<i64>,
        active_s: i64,
    }
    impl Acc {
        fn new(r: &Resolved) -> Self {
            let mut a = Acc {
                start_ms: r.start_ms,
                end_ms: r.end_ms,
                per_key: Default::default(),
                per_app: Default::default(),
                per_detail: Default::default(),
                project_by_key: Default::default(),
                ids: Vec::new(),
                active_s: 0,
            };
            a.add(r);
            a
        }
        fn add(&mut self, r: &Resolved) {
            let s = r.seconds();
            self.end_ms = self.end_ms.max(r.end_ms);
            *self.per_key.entry(r.key()).or_default() += s;
            *self.per_app.entry(r.app.clone()).or_default() += s;
            if let Some(d) = &r.detail {
                *self.per_detail.entry(d.clone()).or_default() += s;
            }
            // UPGRADE the stored origin, don't just keep the first one: every
            // event of a project shares the key `p:<project>`, so a plain
            // `or_insert` froze whichever event happened to come first and the
            // "strongest origin names the slot" rule below could never fire —
            // a block containing an explicitly TAGGED event was still badged
            // as a guess (Neighbour), which is exactly the badge the user
            // reads to decide what to double-check before booking.
            self.project_by_key
                .entry(r.key())
                .and_modify(|(_, o)| {
                    if r.origin.rank() > o.rank() {
                        *o = r.origin;
                    }
                })
                .or_insert_with(|| (r.project.clone(), r.origin));
            self.ids.push(r.id);
            self.active_s += s;
        }
        /// The key that owns the slot: most seconds wins.
        fn dominant(&self) -> (String, i64) {
            self.per_key
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                .map(|(k, v)| (k.clone(), *v))
                .unwrap_or_default()
        }
    }

    let mut slots: Vec<Slot> = Vec::new();
    let mut acc: Option<Acc> = None;

    let finish = |acc: Acc, slots: &mut Vec<Slot>| {
        let (dom_key, dom_s) = acc.dominant();
        let (project, origin) = acc
            .project_by_key
            .get(&dom_key)
            .cloned()
            .unwrap_or((None, Origin::Unassigned));
        // The strongest origin present in the slot names it — a slot built
        // mostly from neighbour-filled events but containing one explicitly
        // tagged event is a tagged slot.
        let origin = acc
            .project_by_key
            .values()
            .filter(|(p, _)| p == &project)
            .map(|(_, o)| *o)
            .max_by_key(|o| o.rank())
            .unwrap_or(origin);
        let mut apps: Vec<SlotApp> = acc
            .per_app
            .into_iter()
            .map(|(app, seconds)| SlotApp { app, seconds })
            .collect();
        apps.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.app.cmp(&b.app)));
        let label = project
            .clone()
            .unwrap_or_else(|| apps.first().map(|a| a.app.clone()).unwrap_or_default());
        let mut details: Vec<(String, i64)> = acc.per_detail.into_iter().collect();
        details.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let description = build_description(&label, &details);
        let span_s = ((acc.end_ms - acc.start_ms).max(0)) / 1000;
        let confidence = if span_s > 0 {
            (dom_s as f64 / span_s as f64).min(1.0)
        } else {
            0.0
        };
        slots.push(Slot {
            start_ms: acc.start_ms,
            end_ms: acc.end_ms,
            project,
            label,
            description,
            origin,
            apps,
            event_ids: acc.ids,
            active_s: acc.active_s,
            span_s,
            confidence,
        });
    };

    for r in events {
        // A long enough idle span is a real break: close whatever is open.
        if r.is_idle {
            if r.seconds() >= params.min_break_s {
                if let Some(a) = acc.take() {
                    finish(a, &mut slots);
                }
            }
            continue;
        }
        // Claude runs alongside the terminal focus; counting it as its own
        // event would double-count the time. It still contributes its project
        // (that is the whole point of the Claude signal) but never extends a
        // slot on its own.
        let Some(cur) = acc.as_mut() else {
            acc = Some(Acc::new(r));
            continue;
        };
        let gap_ms = r.start_ms - cur.end_ms;
        let same_key = cur.per_key.contains_key(&r.key());
        let short_enough_to_absorb = r.seconds() < params.noise_s;
        let bridgeable = gap_ms <= params.bridge_gap_s * 1000;

        if bridgeable && (same_key || short_enough_to_absorb) {
            cur.add(r);
        } else {
            let a = acc.take().unwrap();
            finish(a, &mut slots);
            acc = Some(Acc::new(r));
        }
    }
    if let Some(a) = acc.take() {
        finish(a, &mut slots);
    }

    // Length gate on the MEASURED span, before snapping — snapping rounds
    // outward-ish and would otherwise let a short block through as a long one.
    slots.retain(|s| s.span_s >= params.min_slot_s);

    if params.grid_min > 0 {
        snap_slots(&mut slots, params.grid_min);
    }
    slots
}

/// A per-project daily total — the robust consolidation for the **export**.
/// Unlike [`build_slots`] (a chronological timeline walk that assumes one focus
/// at a time), this groups by project and **unions the intervals**, so it is
/// correct even when several Claude sessions run in **parallel** across
/// different projects (the events overlap heavily — a raw sum would report
/// dozens of hours per day). `seconds` is the overlap-corrected time actually
/// spent on that project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectTotal {
    pub project: String,
    /// Union of the project's intervals (overlaps merged, gaps excluded).
    pub seconds: i64,
    /// First activity on the project that day.
    pub start_ms: i64,
    /// Last activity on the project that day.
    pub end_ms: i64,
    pub apps: Vec<SlotApp>,
    pub description: String,
}

/// Consolidate a day's events into **one row per project**, overlap-corrected.
/// Runs the same project inference as [`build_slots`], then per project unions
/// the intervals (via `union_seconds`). Untagged/unassigned time is skipped
/// (it has no project to book against). Projects below `MIN_TOTAL_S` (1 min) of
/// real time are dropped as noise. Sorted by time desc. Pure + unit-tested.
pub fn project_totals(events: &[TrackEvent], known: &[String], params: &SlotParams) -> Vec<ProjectTotal> {
    const MIN_TOTAL_S: i64 = 60;
    use std::collections::HashMap;
    let resolved = infer_projects(events, known, params);
    struct Agg {
        ivs: Vec<(i64, i64)>,
        apps: HashMap<String, i64>,
        details: HashMap<String, i64>,
    }
    let mut by: HashMap<String, Agg> = HashMap::new();
    for r in &resolved {
        if r.is_idle {
            continue;
        }
        let Some(proj) = r.project.as_ref().filter(|p| !p.is_empty()) else {
            continue; // unassigned time can't be booked against a project
        };
        let a = by.entry(proj.clone()).or_insert_with(|| Agg {
            ivs: Vec::new(),
            apps: HashMap::new(),
            details: HashMap::new(),
        });
        a.ivs.push((r.start_ms, r.end_ms));
        *a.apps.entry(r.app.clone()).or_default() += r.seconds();
        if let Some(d) = &r.detail {
            *a.details.entry(d.clone()).or_default() += r.seconds();
        }
    }
    let mut out: Vec<ProjectTotal> = by
        .into_iter()
        .map(|(project, a)| {
            let seconds = super::union_seconds(a.ivs.clone());
            let start_ms = a.ivs.iter().map(|(s, _)| *s).min().unwrap_or(0);
            let end_ms = a.ivs.iter().map(|(_, e)| *e).max().unwrap_or(0);
            let mut apps: Vec<SlotApp> = a
                .apps
                .into_iter()
                .map(|(app, seconds)| SlotApp { app, seconds })
                .collect();
            apps.sort_by(|x, y| y.seconds.cmp(&x.seconds).then(x.app.cmp(&y.app)));
            let mut details: Vec<(String, i64)> = a.details.into_iter().collect();
            details.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            let description = build_description(&project, &details);
            ProjectTotal { project, seconds, start_ms, end_ms, apps, description }
        })
        .filter(|p| p.seconds >= MIN_TOTAL_S)
        .collect();
    out.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.project.cmp(&b.project)));
    out
}

/// Round every slot's bounds to the nearest grid multiple and drop anything
/// that collapses; then push overlapping neighbours apart so the result can be
/// booked (the target system rejects overlapping entries).
pub fn snap_slots(slots: &mut Vec<Slot>, grid_min: i64) {
    let g = grid_min * 60_000;
    if g <= 0 {
        return;
    }
    for s in slots.iter_mut() {
        s.start_ms = ((s.start_ms as f64 / g as f64).round() as i64) * g;
        s.end_ms = ((s.end_ms as f64 / g as f64).round() as i64) * g;
    }
    slots.retain(|s| s.end_ms > s.start_ms);
    slots.sort_by_key(|s| s.start_ms);
    // Resolve collisions created by rounding: the earlier slot keeps its start,
    // the later one begins where the earlier ends.
    for i in 1..slots.len() {
        let prev_end = slots[i - 1].end_ms;
        if slots[i].start_ms < prev_end {
            slots[i].start_ms = prev_end;
        }
    }
    slots.retain(|s| s.end_ms > s.start_ms);
    for s in slots.iter_mut() {
        s.span_s = (s.end_ms - s.start_ms) / 1000;
        if s.span_s > 0 {
            s.confidence = s.confidence.min(1.0);
        }
    }
}

/// Build a booking description: the project/app label plus the most-used window
/// titles, deduplicated and capped so the line stays readable in a timesheet.
fn build_description(label: &str, details: &[(String, i64)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (d, _) in details.iter().take(3) {
        let clean = d.trim();
        if clean.is_empty() || clean.eq_ignore_ascii_case(label) {
            continue;
        }
        let short: String = clean.chars().take(60).collect();
        if !parts.iter().any(|p| p == &short) {
            parts.push(short);
        }
    }
    if parts.is_empty() {
        label.to_string()
    } else {
        format!("{label} — {}", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed epoch base for the fixtures. It MUST sit on a 15-minute boundary:
    /// snapping rounds absolute epoch milliseconds (which lines up with local
    /// quarter hours in every real timezone, all of whose offsets are multiples
    /// of 15 minutes), so an unaligned base would shift every expectation by an
    /// arbitrary offset.
    const T0: i64 = 1_700_000_100_000;

    fn ev(id: i64, from_min: i64, to_min: i64, app: &str, project: Option<&str>) -> TrackEvent {
        TrackEvent {
            id,
            session_id: 1,
            app_name: app.to_string(),
            app_id: None,
            window_title: None,
            url: None,
            host: None,
            category: None,
            project: project.map(str::to_string),
            source: "focus".into(),
            is_idle: false,
            started_at: T0 + from_min * 60_000,
            ended_at: Some(T0 + to_min * 60_000),
            duration_s: Some((to_min - from_min) * 60),
        }
    }

    fn with_title(mut e: TrackEvent, title: &str) -> TrackEvent {
        e.window_title = Some(title.to_string());
        e
    }

    fn idle(id: i64, from_min: i64, to_min: i64) -> TrackEvent {
        let mut e = ev(id, from_min, to_min, "idle", None);
        e.is_idle = true;
        e
    }

    fn secs(from_min: i64, to_min: i64) -> i64 {
        (to_min - from_min) * 60
    }

    // ── project_totals (overlap-corrected export consolidation) ──────────────

    #[test]
    fn project_totals_union_corrects_parallel_overlap() {
        // Two overlapping "parallel Claude session" events on the SAME project
        // (00:00–00:30 and 00:10–00:40) → union is 40 min, NOT the 60-min sum.
        let events = vec![
            ev(1, 0, 30, "Claude Code", Some("alpha")),
            ev(2, 10, 40, "Claude Code", Some("alpha")),
        ];
        let totals = project_totals(&events, &[], &SlotParams::default());
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].project, "alpha");
        assert_eq!(totals[0].seconds, 40 * 60); // union, overlap removed
        assert_eq!(totals[0].start_ms, T0);
    }

    #[test]
    fn project_totals_groups_distinct_projects_and_survives_fragmentation() {
        // A fragmented, interleaved multi-project day (the real failure case):
        // 3 min here, 4 min there — the timeline walk + 15-min floor would drop
        // all of it, but per-project totals keep every project's real time.
        let events = vec![
            ev(1, 0, 3, "Claude Code", Some("alpha")),
            ev(2, 3, 7, "Claude Code", Some("beta")),
            ev(3, 7, 10, "Claude Code", Some("alpha")),
            ev(4, 10, 12, "Claude Code", Some("beta")),
            idle(5, 12, 30),
        ];
        let totals = project_totals(&events, &[], &SlotParams::default());
        assert_eq!(totals.len(), 2);
        // alpha: 3 + 3 = 6 min; beta: 4 + 2 = 6 min (sorted desc, tie → name)
        let alpha = totals.iter().find(|p| p.project == "alpha").unwrap();
        let beta = totals.iter().find(|p| p.project == "beta").unwrap();
        assert_eq!(alpha.seconds, 6 * 60);
        assert_eq!(beta.seconds, 6 * 60);
    }

    #[test]
    fn project_totals_skips_unassigned_and_sub_minute_noise() {
        let events = vec![
            ev(1, 0, 10, "iTerm2", None), // no project → skipped
            ev(2, 10, 10, "Claude Code", Some("tiny")), // 0 s → below floor
        ];
        assert!(project_totals(&events, &[], &SlotParams::default()).is_empty());
    }

    #[test]
    fn project_totals_union_excludes_the_gap_between_a_projects_intervals() {
        // One project, two intervals far apart (0–10 min and 40–50 min): the
        // total is the sum of the two (20 min), NOT the 50-min span — the idle
        // gap in between is not work.
        let events = vec![
            ev(1, 0, 10, "Claude Code", Some("alpha")),
            ev(2, 40, 50, "Claude Code", Some("alpha")),
        ];
        let totals = project_totals(&events, &[], &SlotParams::default());
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].seconds, 20 * 60); // 10 + 10, not 50
        assert_eq!(totals[0].start_ms, T0); // first activity
        assert_eq!(totals[0].end_ms, T0 + 50 * 60_000); // last activity
    }

    #[test]
    fn project_totals_counts_title_token_inferred_time() {
        // An UNTAGGED terminal event whose window title carries a known project
        // token ("kiez-finder-bug-fixes" → kiez-finder) is attributed to that
        // project — the "cleverly link untagged time" path, all the way through
        // to a bookable total. `known` supplies the token (as the project map
        // would at runtime).
        let events = vec![
            with_title(ev(1, 0, 5, "iTerm2", None), "✳ kiez-finder-bug-fixes"),
            ev(2, 5, 12, "Claude Code", Some("kiez-finder")),
        ];
        let known = vec!["kiez-finder".to_string()];
        let totals = project_totals(&events, &known, &SlotParams::default());
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].project, "kiez-finder");
        // 5 min (title-inferred terminal) + 7 min (Claude) = 12 min union.
        assert_eq!(totals[0].seconds, 12 * 60);
    }

    #[test]
    fn project_totals_do_not_double_count_overlapping_claude_and_terminal() {
        // While Claude runs on a project the terminal is focused on the SAME
        // project — the two events overlap in wall-clock. The union must count
        // that shared time ONCE (not terminal + Claude summed).
        let mut claude = ev(1, 0, 20, "Claude Code", Some("alpha"));
        claude.source = "claude".into();
        let terminal = with_title(ev(2, 0, 20, "iTerm2", None), "✳ alpha");
        let totals = project_totals(&[claude, terminal], &["alpha".to_string()], &SlotParams::default());
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].project, "alpha");
        assert_eq!(totals[0].seconds, 20 * 60); // 20 min once, not 40
    }

    #[test]
    fn project_totals_are_sorted_by_time_descending() {
        let events = vec![
            ev(1, 0, 3, "Claude Code", Some("small")),   // 3 min
            ev(2, 3, 18, "Claude Code", Some("big")),    // 15 min
            ev(3, 18, 26, "Claude Code", Some("medium")), // 8 min
        ];
        let totals = project_totals(&events, &[], &SlotParams::default());
        let order: Vec<&str> = totals.iter().map(|p| p.project.as_str()).collect();
        assert_eq!(order, vec!["big", "medium", "small"]);
    }

    // ── normalize / tokens / matching ────────────────────────────────────────

    #[test]
    fn normalize_token_strips_separators_and_case() {
        assert_eq!(normalize_token("Kiez-Finder"), "kiezfinder");
        assert_eq!(normalize_token("kiez_finder"), "kiezfinder");
        assert_eq!(normalize_token("  spaced  "), "spaced");
        assert_eq!(normalize_token("Übung"), "übung");
    }

    #[test]
    fn tokens_drop_noise_that_would_match_anything() {
        let t = tokens_of("a de io kiez finder");
        assert_eq!(t, vec!["kiez", "finder"]);
    }

    #[test]
    fn project_matches_when_all_its_tokens_appear() {
        let known = vec!["kiez-finder".to_string(), "bcsbook".to_string()];
        assert_eq!(
            match_project_in_text("src/main.rs — kiez finder — Cursor", &known),
            Some("kiez-finder".to_string()),
        );
        assert_eq!(match_project_in_text("Inbox — Mail", &known), None);
    }

    #[test]
    fn a_partial_name_does_not_match() {
        let known = vec!["kiez-finder".to_string()];
        // Only half of the project name is present.
        assert_eq!(match_project_in_text("kiez tour planning", &known), None);
    }

    #[test]
    fn the_more_specific_project_wins() {
        let known = vec!["inspector".to_string(), "inspector-rust".to_string()];
        assert_eq!(
            match_project_in_text("inspector rust — slots.rs", &known),
            Some("inspector-rust".to_string()),
        );
    }

    #[test]
    fn known_projects_dedups_case_insensitively_and_keeps_extras() {
        let evs = vec![ev(1, 0, 1, "A", Some("Alpha")), ev(2, 1, 2, "B", Some("alpha"))];
        let k = known_projects(&evs, &["Beta".to_string(), "Alpha".to_string()]);
        assert_eq!(k, vec!["Alpha".to_string(), "Beta".to_string()]);
    }

    // ── inference ────────────────────────────────────────────────────────────

    #[test]
    fn an_explicit_tag_wins_and_is_reported_as_such() {
        let evs = vec![ev(1, 0, 10, "Cursor", Some("alpha"))];
        let r = infer_projects(&evs, &[], &SlotParams::default());
        assert_eq!(r[0].project.as_deref(), Some("alpha"));
        assert_eq!(r[0].origin, Origin::Tagged);
    }

    #[test]
    fn the_strongest_origin_in_a_slot_names_it() {
        // REGRESSION (found 2026-08-15): every event of one project shares the
        // key `p:<project>`, so keeping the FIRST origin per key froze the
        // weakest one and the "strongest wins" rule below could never fire —
        // a block containing an explicitly tagged event was still badged as a
        // guess, and that badge is what the user reads to decide which rows to
        // double-check before booking.
        let evs = vec![
            with_title(ev(1, 0, 600, "Safari", None), "alpha docs"), // guessed
            ev(2, 600, 1200, "Cursor", Some("alpha")),               // explicitly tagged
        ];
        let resolved = infer_projects(&evs, &["alpha".to_string()], &SlotParams::default());
        let slots = build_slots(&resolved, &SlotParams::default());
        assert_eq!(slots.len(), 1, "one contiguous project block");
        assert_eq!(slots[0].origin, Origin::Tagged, "the tag outranks the guess");
    }

    #[test]
    fn a_title_token_assigns_the_project() {
        let evs = vec![with_title(ev(1, 0, 10, "Cursor", None), "slots.rs — inspector-rust")];
        let known = vec!["inspector-rust".to_string()];
        let r = infer_projects(&evs, &known, &SlotParams::default());
        assert_eq!(r[0].project.as_deref(), Some("inspector-rust"));
        assert_eq!(r[0].origin, Origin::Title);
    }

    #[test]
    fn untagged_time_between_two_blocks_of_one_project_is_inherited() {
        let evs = vec![
            ev(1, 0, 10, "Cursor", Some("alpha")),
            ev(2, 10, 12, "Safari", None), // a quick lookup
            ev(3, 12, 20, "Cursor", Some("alpha")),
        ];
        let r = infer_projects(&evs, &[], &SlotParams::default());
        assert_eq!(r[1].project.as_deref(), Some("alpha"));
        assert_eq!(r[1].origin, Origin::Neighbour);
    }

    #[test]
    fn a_break_stops_the_project_from_bleeding_across_it() {
        let evs = vec![
            ev(1, 0, 10, "Cursor", Some("alpha")),
            idle(2, 10, 40),
            ev(3, 40, 50, "Safari", None),
        ];
        let r = infer_projects(&evs, &[], &SlotParams::default());
        let after = r.iter().find(|x| x.id == 3).unwrap();
        assert_eq!(after.project, None, "must not inherit across a break");
    }

    #[test]
    fn distant_untagged_time_is_not_claimed() {
        let evs = vec![
            ev(1, 0, 10, "Cursor", Some("alpha")),
            ev(2, 60, 70, "Safari", None), // 50 minutes later
        ];
        let r = infer_projects(&evs, &[], &SlotParams::default());
        assert_eq!(r[1].project, None);
    }

    #[test]
    fn neighbour_inference_can_be_switched_off() {
        let evs = vec![
            ev(1, 0, 10, "Cursor", Some("alpha")),
            ev(2, 10, 12, "Safari", None),
        ];
        let p = SlotParams { neighbour_gap_s: 0, ..Default::default() };
        let r = infer_projects(&evs, &[], &p);
        assert_eq!(r[1].project, None);
    }

    #[test]
    fn events_without_an_end_are_skipped_rather_than_counted_to_forever() {
        let mut open = ev(1, 0, 10, "Cursor", None);
        open.ended_at = None;
        let r = infer_projects(&[open], &[], &SlotParams::default());
        assert!(r.is_empty());
    }

    // ── slot building ────────────────────────────────────────────────────────

    #[test]
    fn the_fragmentation_case_becomes_one_slot() {
        // The screenshot case: rapid switching between three apps on one
        // project — 30 fragments of ~30 s over 15 minutes.
        let mut evs = Vec::new();
        let apps = ["Cursor", "Claude", "Safari"];
        for i in 0..30i64 {
            let f = i / 2;
            evs.push(ev(i + 1, f, f + 1, apps[(i % 3) as usize], Some("alpha")));
        }
        let r = infer_projects(&evs, &[], &SlotParams::default());
        let slots = build_slots(&r, &SlotParams::default());
        assert_eq!(slots.len(), 1, "must consolidate to a single slot");
        assert_eq!(slots[0].project.as_deref(), Some("alpha"));
        assert_eq!(slots[0].apps.len(), 3, "all three apps are listed as contributors");
    }

    #[test]
    fn a_short_foreign_interruption_does_not_split_the_block() {
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            ev(2, 60, 61, "Slack", Some("beta")), // 60 s glance
            ev(3, 61, 120, "Cursor", Some("alpha")),
        ];
        let p = SlotParams { neighbour_gap_s: 0, ..Default::default() };
        let slots = build_slots(&infer_projects(&evs, &[], &p), &p);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].project.as_deref(), Some("alpha"));
    }

    #[test]
    fn a_real_project_switch_does_split() {
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            ev(2, 60, 120, "Cursor", Some("beta")),
        ];
        let p = SlotParams { neighbour_gap_s: 0, ..Default::default() };
        let slots = build_slots(&infer_projects(&evs, &[], &p), &p);
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].project.as_deref(), Some("alpha"));
        assert_eq!(slots[1].project.as_deref(), Some("beta"));
    }

    #[test]
    fn a_long_break_splits_the_day() {
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            idle(2, 60, 120), // one hour of lunch
            ev(3, 120, 180, "Cursor", Some("alpha")),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 2, "lunch must separate morning from afternoon");
    }

    #[test]
    fn a_brief_idle_dip_does_not_split() {
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            idle(2, 60, 62), // two minutes, under the break threshold
            ev(3, 62, 120, "Cursor", Some("alpha")),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 1);
    }

    #[test]
    fn slots_below_the_minimum_are_dropped() {
        let evs = vec![ev(1, 0, 5, "Cursor", Some("alpha"))]; // 5 minutes
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert!(slots.is_empty(), "5 minutes is under the 15-minute floor");
    }

    #[test]
    fn the_gate_uses_measured_length_so_snapping_cannot_smuggle_a_slot_through() {
        // 20 minutes measured, floor 30 → must be dropped even though snapping
        // outward would have produced a 45-minute block.
        let evs = vec![ev(1, 3, 23, "Cursor", Some("alpha"))];
        let p = SlotParams { min_slot_s: 1800, ..Default::default() };
        let slots = build_slots(&infer_projects(&evs, &[], &p), &p);
        assert!(slots.is_empty());
    }

    #[test]
    fn unassigned_time_still_consolidates_by_app() {
        let mut evs = Vec::new();
        for i in 0..40i64 {
            evs.push(ev(i + 1, i, i + 1, "Mail", None));
        }
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].project, None);
        assert_eq!(slots[0].label, "Mail");
        assert_eq!(slots[0].origin, Origin::Unassigned);
    }

    // ── snapping ─────────────────────────────────────────────────────────────

    #[test]
    fn bounds_snap_to_the_nearest_quarter_and_do_not_inflate() {
        // 09:03–11:47 measured → 09:00–11:45, i.e. 2.75 h, not 3.0.
        let evs = vec![ev(1, 3, 107, "Cursor", Some("alpha"))];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].start_ms, T0);
        assert_eq!(slots[0].span_s, secs(0, 105));
    }

    #[test]
    fn snapping_never_produces_overlapping_slots() {
        // Two adjacent blocks whose rounded bounds would collide.
        let evs = vec![
            ev(1, 0, 52, "Cursor", Some("alpha")),
            ev(2, 52, 120, "Cursor", Some("beta")),
        ];
        let p = SlotParams { neighbour_gap_s: 0, ..Default::default() };
        let slots = build_slots(&infer_projects(&evs, &[], &p), &p);
        assert_eq!(slots.len(), 2);
        assert!(slots[0].end_ms <= slots[1].start_ms, "slots must not overlap");
    }

    #[test]
    fn snapping_can_be_disabled_for_exact_bounds() {
        let evs = vec![ev(1, 3, 107, "Cursor", Some("alpha"))];
        let p = SlotParams { grid_min: 0, ..Default::default() };
        let slots = build_slots(&infer_projects(&evs, &[], &p), &p);
        assert_eq!(slots[0].span_s, secs(3, 107));
    }

    // ── descriptions & confidence ────────────────────────────────────────────

    #[test]
    fn the_description_names_the_project_and_its_main_titles() {
        let evs = vec![
            with_title(ev(1, 0, 40, "Cursor", Some("alpha")), "slots.rs"),
            with_title(ev(2, 40, 60, "Cursor", Some("alpha")), "mod.rs"),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert!(slots[0].description.starts_with("alpha — "));
        assert!(slots[0].description.contains("slots.rs"));
    }

    #[test]
    fn a_mixed_block_reports_low_confidence() {
        // Half alpha, half absorbed noise from other projects.
        let evs = vec![
            ev(1, 0, 30, "Cursor", Some("alpha")),
            ev(2, 30, 31, "Slack", Some("beta")),
            ev(3, 31, 32, "Mail", Some("gamma")),
            ev(4, 32, 60, "Cursor", Some("alpha")),
        ];
        let p = SlotParams { neighbour_gap_s: 0, ..Default::default() };
        let slots = build_slots(&infer_projects(&evs, &[], &p), &p);
        assert_eq!(slots.len(), 1);
        assert!(slots[0].confidence < 1.0);
        assert!(slots[0].confidence > 0.9, "still dominated by alpha");
    }

    #[test]
    fn event_ids_are_kept_as_an_audit_trail() {
        let evs = vec![
            ev(7, 0, 30, "Cursor", Some("alpha")),
            ev(9, 30, 60, "Cursor", Some("alpha")),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots[0].event_ids, vec![7, 9]);
    }

    #[test]
    fn an_empty_day_yields_no_slots() {
        assert!(build_slots(&[], &SlotParams::default()).is_empty());
    }

    #[test]
    fn a_gap_wider_than_the_bridge_splits_even_the_same_project() {
        // Same project on both sides, but 20 minutes of nothing in between (no
        // idle event was recorded — the machine was simply not used). Bridging
        // that would book 20 minutes nobody worked.
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            ev(2, 80, 140, "Cursor", Some("alpha")),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 2, "the unexplained gap must not be booked");
        assert_eq!(slots[0].span_s, secs(0, 60));
        assert_eq!(slots[1].span_s, secs(80, 140));
        // A gap *within* the bridge (5 min) is one slot again.
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            ev(2, 64, 124, "Cursor", Some("alpha")),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 1);
    }

    #[test]
    fn a_short_idle_dip_is_bridged_but_never_counted_as_worked_time() {
        // `active_s` is what the slot actually measured; `span_s` is the booked
        // wall clock. The dip belongs to the span, not to the measurement.
        let evs = vec![
            ev(1, 0, 60, "Cursor", Some("alpha")),
            idle(2, 60, 62),
            ev(3, 62, 120, "Cursor", Some("alpha")),
        ];
        let slots = build_slots(&infer_projects(&evs, &[], &SlotParams::default()), &SlotParams::default());
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].active_s, secs(0, 60) + secs(62, 120), "the 2 idle minutes are not work");
        assert_eq!(slots[0].span_s, secs(0, 120));
        assert_eq!(slots[0].event_ids, vec![1, 3], "the idle event is not part of the audit trail");
    }

    #[test]
    fn the_description_drops_a_title_that_only_repeats_the_label_and_caps_the_rest() {
        // Only the three heaviest titles make the line, an echo of the label is
        // noise, and a very long title is truncated so the timesheet row stays
        // readable.
        let long = "x".repeat(80);
        let details = vec![
            ("alpha".to_string(), 500), // echoes the label → dropped
            ("slots.rs".to_string(), 400),
            (long.clone(), 300),
            ("mod.rs".to_string(), 200), // 4th by weight → outside the cap
        ];
        let d = build_description("alpha", &details);
        assert!(d.starts_with("alpha — "));
        assert!(d.contains("slots.rs"));
        assert!(!d.contains("mod.rs"), "only the top three details are considered");
        assert!(d.contains(&"x".repeat(60)) && !d.contains(&"x".repeat(61)), "titles cap at 60 chars");
        // A label match is case-insensitive, and with nothing left the label
        // alone is the description (never a dangling em dash).
        assert_eq!(build_description("Alpha", &[("alpha".into(), 1)]), "Alpha");
        assert_eq!(build_description("Mail", &[]), "Mail");
    }

    #[test]
    fn known_projects_ignores_blank_names_that_would_match_everything() {
        // An event tagged with "" or "   " must never become a project token —
        // `tokens_of` would yield nothing and `all()` over an empty token list
        // is vacuously true, i.e. it would claim every window title.
        let evs = vec![
            ev(1, 0, 1, "A", Some("   ")),
            ev(2, 1, 2, "B", Some("")),
            ev(3, 2, 3, "C", Some("alpha")),
        ];
        let k = known_projects(&evs, &["".to_string(), "  ".to_string()]);
        assert_eq!(k, vec!["alpha".to_string()]);
    }

    #[test]
    fn snapping_drops_a_slot_that_rounding_collapsed_into_its_neighbour() {
        // Two blocks that both round onto the same quarter hour: the earlier one
        // keeps the slot, the later one would start where it ends — a zero (or
        // negative) length booking, which must be removed rather than emitted.
        let mut slots = vec![
            slot_at(T0, T0 + 14 * 60_000, "alpha"),   // → 00:00–00:15
            slot_at(T0 + 15 * 60_000, T0 + 16 * 60_000, "beta"), // → 00:15–00:15
        ];
        snap_slots(&mut slots, 15);
        assert_eq!(slots.len(), 1, "the collapsed slot is dropped");
        assert_eq!(slots[0].label, "alpha");
        assert_eq!(slots[0].span_s, 15 * 60, "span_s is recomputed from the snapped bounds");
    }

    /// A bare slot for the snapping tests (the fields snapping touches).
    fn slot_at(start_ms: i64, end_ms: i64, label: &str) -> Slot {
        Slot {
            start_ms,
            end_ms,
            project: Some(label.to_string()),
            label: label.to_string(),
            description: label.to_string(),
            origin: Origin::Tagged,
            apps: Vec::new(),
            event_ids: Vec::new(),
            active_s: (end_ms - start_ms) / 1000,
            span_s: (end_ms - start_ms) / 1000,
            confidence: 1.0,
        }
    }
}

