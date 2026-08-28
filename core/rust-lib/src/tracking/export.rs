//! Timesheet export — flat **CSV** and a **single self-contained HTML** report
//! (CSS + charts inline, zero external requests, light and print-first,
//! offline-viewable). Both documents use the shared [`crate::report_style`].
//! Charts are server-rendered inline **SVG** (no JS needed to view). Pure
//! builders over already-decrypted [`TrackEvent`]s so they're unit-testable.

use super::db::TrackEvent;
use crate::report_style as rs;
use chrono::{Local, TimeZone};
use std::collections::HashMap;

const FOOTER: &str = "© 2026 Martin Pfeffer | celox.io";

/// The few rules the shared stylesheet cannot know about: the donut's two-column
/// layout, the collapsible per-app details, and the grand-total line. Appended
/// to [`rs::css`] rather than replacing it, so the common base stays the base.
const EXTRA_CSS: &str = r#"
.ts-donut { display: flex; align-items: center; gap: 22px }
.ts-donut .lg { flex: 1; min-width: 0 }
.ts-donut .lg .row { display: flex; align-items: center; gap: 9px; padding: 3px 0; font-size: 13px }
.ts-donut .lg i { width: 9px; height: 9px; border-radius: 2px; flex: none }
.ts-donut .lg .k { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.ts-donut .lg .v { color: var(--muted); font-variant-numeric: tabular-nums }
.ts-cols { display: grid; grid-template-columns: 1fr 1fr; gap: 30px }
.rp-total { text-align: right; font-weight: 640; margin: 18px 0 0 }
.rp-total span { font-variant-numeric: tabular-nums }
details { border-bottom: 1px solid #f2f4f7 }
summary { display: flex; align-items: center; gap: 10px; cursor: pointer; padding: 8px 0; list-style: none }
summary::-webkit-details-marker { display: none }
summary::before { content: "\25B8"; color: var(--muted) }
details[open] summary::before { content: "\25BE" }
summary .n { flex: 1; font-weight: 600 }
summary .dur { color: var(--muted); font-variant-numeric: tabular-nums }
details > table { margin: 0 0 10px 18px }
.ts-src { color: var(--muted); font-size: 11px }
@media print { .ts-cols { gap: 20px } details[open] summary::before { content: "\25BE" } }
"#;

fn effective_dur_s(e: &TrackEvent, now: i64) -> i64 {
    e.duration_s
        .unwrap_or_else(|| ((e.ended_at.unwrap_or(now) - e.started_at).max(0)) / 1000)
}

fn fmt_dur(secs: i64) -> String {
    let s = secs.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{s}s")
    }
}

fn local_date(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

// ── CSV ──────────────────────────────────────────────────────────────────────

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Flat CSV: `date,start,end,duration_min,app,category,project,host,title,source,idle`.
pub fn csv(
    events: &[TrackEvent],
    now: i64,
    project_days: &[(String, Vec<crate::tracking::slots::ProjectTotal>)],
) -> String {
    // Consolidated per-project summary FIRST — it's the point of the export, so
    // it must not be buried under hundreds of raw-event rows (the "still not
    // consolidated" report was just the block sitting at the very bottom).
    let mut out = project_section_csv(project_days);
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("# Raw events\n");
    out.push_str("date,start,end,duration_min,app,category,project,host,title,source,idle\n");
    for e in events {
        let dur_min = format!("{:.1}", effective_dur_s(e, now) as f64 / 60.0);
        let row = [
            local_date(e.started_at),
            local_time(e.started_at),
            e.ended_at.map(local_time).unwrap_or_default(),
            dur_min,
            e.app_name.clone(),
            e.category.clone().unwrap_or_default(),
            e.project.clone().unwrap_or_default(),
            e.host.clone().unwrap_or_default(),
            e.window_title.clone().unwrap_or_default(),
            e.source.clone(),
            if e.is_idle { "1".into() } else { "0".into() },
        ];
        out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

/// The consolidated **per-project** totals as a clearly delimited CSV block at
/// the TOP of the file (before the raw events — it's what the export is for).
/// `hours` is the overlap-corrected union of the project's time, so it stays
/// correct even with parallel Claude sessions. Empty when nothing consolidated.
fn project_section_csv(project_days: &[(String, Vec<crate::tracking::slots::ProjectTotal>)]) -> String {
    if project_days.iter().all(|(_, p)| p.is_empty()) {
        return String::new();
    }
    let mut out = String::from("# Consolidated per project (overlap-corrected)\ndate,project,hours,first,last,apps\n");
    for (date, projects) in project_days {
        for p in projects {
            let apps = p.apps.iter().take(3).map(|a| a.app.clone()).collect::<Vec<_>>().join(", ");
            let row = [
                date.clone(),
                p.project.clone(),
                hours2(p.seconds),
                local_time(p.start_ms),
                local_time(p.end_ms),
                apps,
            ];
            out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
            out.push('\n');
        }
    }
    out
}

// ── Project export (customer-facing: when · how long · on what) ──────────────

/// Detail level of the project export.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// One total per project.
    Summary,
    /// One total per project per day.
    Daily,
    /// Every entry (date · time · duration · activity).
    Full,
}

impl Detail {
    pub fn parse(s: &str) -> Detail {
        match s {
            "summary" => Detail::Summary,
            "daily" => Detail::Daily,
            _ => Detail::Full,
        }
    }
}

/// Billable events for the project export: active, non-Claude, with a non-empty
/// project — optionally filtered to a single `project` (None/"" = all). Claude
/// time is excluded so it can't double-bill terminal focus. Sorted by project,
/// then start time.
fn billable_by_project<'a>(
    events: &'a [TrackEvent],
    project: Option<&str>,
) -> Vec<&'a TrackEvent> {
    let only = project.filter(|p| !p.is_empty());
    let mut v: Vec<&TrackEvent> = events
        .iter()
        .filter(|e| {
            !e.is_idle
                && e.source != "claude"
                && e.project.as_deref().map(|p| !p.is_empty()).unwrap_or(false)
                && only.map(|o| e.project.as_deref() == Some(o)).unwrap_or(true)
        })
        .collect();
    v.sort_by(|a, b| a.project.cmp(&b.project).then(a.started_at.cmp(&b.started_at)));
    v
}

fn activity(e: &TrackEvent) -> String {
    match (&e.window_title, &e.host) {
        (Some(t), _) if !t.is_empty() => format!("{} — {}", e.app_name, t),
        (_, Some(h)) if !h.is_empty() => format!("{} — {}", e.app_name, h),
        _ => e.app_name.clone(),
    }
}

fn min1(secs: i64) -> String {
    format!("{:.1}", secs as f64 / 60.0)
}

/// CSV for the project export. Columns depend on `detail`:
/// - Summary: `project,duration_min`
/// - Daily:   `project,date,duration_min`
/// - Full:    `project,date,start,end,duration_min,app,activity`
pub fn project_csv(events: &[TrackEvent], now: i64, detail: Detail, project: Option<&str>) -> String {
    let billable = billable_by_project(events, project);
    match detail {
        Detail::Summary => {
            let mut out = String::from("project,duration_min\n");
            let mut i = 0;
            while i < billable.len() {
                let proj = billable[i].project.clone().unwrap_or_default();
                let mut total = 0i64;
                while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                    total += effective_dur_s(billable[i], now);
                    i += 1;
                }
                out.push_str(&format!("{},{}\n", csv_field(&proj), min1(total)));
            }
            out
        }
        Detail::Daily => {
            let mut out = String::from("project,date,duration_min\n");
            // billable is sorted by project then start → group (project,date).
            let mut i = 0;
            while i < billable.len() {
                let proj = billable[i].project.clone().unwrap_or_default();
                // accumulate per date within this project
                use std::collections::BTreeMap;
                let mut per_day: BTreeMap<String, i64> = BTreeMap::new();
                while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                    let e = billable[i];
                    *per_day.entry(local_date(e.started_at)).or_default() += effective_dur_s(e, now);
                    i += 1;
                }
                for (date, total) in per_day {
                    out.push_str(&format!("{},{},{}\n", csv_field(&proj), date, min1(total)));
                }
            }
            out
        }
        Detail::Full => {
            let mut out =
                String::from("project,date,start,end,duration_min,app,activity\n");
            for e in billable {
                let row = [
                    e.project.clone().unwrap_or_default(),
                    local_date(e.started_at),
                    local_time(e.started_at),
                    e.ended_at.map(local_time).unwrap_or_default(),
                    min1(effective_dur_s(e, now)),
                    e.app_name.clone(),
                    e.window_title.clone().or_else(|| e.host.clone()).unwrap_or_default(),
                ];
                out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
                out.push('\n');
            }
            out
        }
    }
}

/// Self-contained, printable HTML project report at the chosen `detail`,
/// optionally filtered to a single `project`. Per-project sections + a grand
/// total. For handing a client a "when · how long · on what" list (or just a
/// per-day / total summary).
pub fn project_html(
    events: &[TrackEvent],
    from: i64,
    to: i64,
    now: i64,
    detail: Detail,
    project: Option<&str>,
) -> String {
    use std::collections::BTreeMap;
    let billable = billable_by_project(events, project);
    let range = if local_date(from) == local_date(to.saturating_sub(1)) {
        local_date(from)
    } else {
        format!("{} – {}", local_date(from), local_date(to.saturating_sub(1)))
    };
    let grand: i64 = billable.iter().map(|e| effective_dur_s(e, now)).sum();
    let scope = project
        .filter(|p| !p.is_empty())
        .map(|p| format!(" · {}", esc(p)))
        .unwrap_or_default();

    let mut sections = String::new();
    if detail == Detail::Summary {
        // Single table: project → total.
        // Dieselbe Form wie loc: Farbchip + Anteils-Spur BEIM Namen, damit das
        // Auge für die Proportion nicht in eine eigene Spalte wandern muss.
        let mut totals: Vec<(String, i64)> = Vec::new();
        let mut i = 0;
        while i < billable.len() {
            let proj = billable[i].project.clone().unwrap_or_default();
            let mut total = 0i64;
            while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                total += effective_dur_s(billable[i], now);
                i += 1;
            }
            totals.push((proj, total));
        }
        let sum: i64 = totals.iter().map(|(_, s)| *s).sum();
        let share_of = |s: i64| if sum > 0 { s as f64 / sum as f64 } else { 0.0 };
        let parts: Vec<(String, f64, String)> = totals
            .iter()
            .map(|(p, s)| (esc(p), share_of(*s), rs::series_color(p).to_string()))
            .collect();
        let rows: String = totals
            .iter()
            .map(|(p, s)| {
                format!(
                    "<tr><td>{}</td><td class=\"rp-num\">{}</td><td class=\"rp-num rp-dim\">{}</td></tr>",
                    rs::name_cell(rs::series_color(p), &esc(p), share_of(*s)),
                    fmt_dur(*s),
                    rs::pct(share_of(*s)),
                )
            })
            .collect();
        sections = format!(
            "<section><h2>Nach Projekt</h2>{bar}<table><thead><tr><th>Projekt</th>\
             <th class=\"rp-num\">Dauer</th><th class=\"rp-num\">Anteil</th></tr></thead>\
             <tbody>{rows}</tbody></table></section>",
            bar = rs::share_bar(&parts),
        );
    } else {
        let mut i = 0;
        while i < billable.len() {
            let proj = billable[i].project.clone().unwrap_or_default();
            let mut rows = String::new();
            let mut ptotal = 0i64;
            if detail == Detail::Daily {
                let mut per_day: BTreeMap<String, i64> = BTreeMap::new();
                while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                    let e = billable[i];
                    *per_day.entry(local_date(e.started_at)).or_default() += effective_dur_s(e, now);
                    i += 1;
                }
                for (date, d) in &per_day {
                    ptotal += d;
                    rows.push_str(&format!(
                        "<tr><td>{}</td><td class=\"rp-num\">{}</td></tr>",
                        date,
                        fmt_dur(*d)
                    ));
                }
                sections.push_str(&format!(
                    "<section><h2>{}</h2><table><thead><tr><th>Datum</th>\
                     <th class=\"rp-num\">Dauer</th></tr></thead><tbody>{2}</tbody>\
                     <tfoot><tr><td>Gesamt</td><td class=\"rp-num\">{1}</td></tr></tfoot></table></section>",
                    esc(&proj), fmt_dur(ptotal), rows
                ));
            } else {
                while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                    let e = billable[i];
                    let d = effective_dur_s(e, now);
                    ptotal += d;
                    rows.push_str(&format!(
                        "<tr><td>{}</td><td class=\"rp-text\">{}–{}</td><td class=\"rp-num\">{}</td><td class=\"rp-text\">{}</td></tr>",
                        local_date(e.started_at),
                        local_time(e.started_at),
                        e.ended_at.map(local_time).unwrap_or_default(),
                        fmt_dur(d),
                        esc(&activity(e)),
                    ));
                    i += 1;
                }
                sections.push_str(&format!(
                    "<section><h2>{}</h2><table><thead><tr><th>Datum</th><th class=\"rp-text\">Zeit</th>\
                     <th class=\"rp-num\">Dauer</th><th class=\"rp-text\">Tätigkeit</th></tr></thead><tbody>{2}</tbody>\
                     <tfoot><tr><td colspan=2>Gesamt</td><td class=\"rp-num\">{1}</td><td></td></tr></tfoot></table></section>",
                    esc(&proj), fmt_dur(ptotal), rows
                ));
            }
        }
    }
    if billable.is_empty() {
        sections = "<p class=\"rp-empty\">In diesem Zeitraum ist keine Zeit einem Projekt zugeordnet. Zuordnen: ein Fenster auf der Tagesleiste ziehen.</p>".into();
    }

    let body = format!(
        r#"{sections}
<p class="rp-total">Gesamt <span>{grand}</span></p>"#,
        sections = sections,
        grand = fmt_dur(grand),
    );
    let subject = format!("{}{}", esc(&range), scope);
    rs::shell("Zeiterfassung", "Projekt-Report", &subject, &body, FOOTER)
        .replace("</style>", &format!("{EXTRA_CSS}\n</style>"))
}

// ── HTML ─────────────────────────────────────────────────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn buckets_sorted(map: HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// One donut ring-sector path (mirrors the frontend `donutSegmentPath`).
fn donut_path(cx: f64, cy: f64, r_out: f64, r_in: f64, a0: f64, a1: f64) -> String {
    let polar = |r: f64, deg: f64| {
        let a = (deg - 90.0) * std::f64::consts::PI / 180.0;
        (cx + r * a.cos(), cy + r * a.sin())
    };
    let sweep = a1 - a0;
    let eps = if sweep >= 360.0 { -0.001 } else { 0.0 };
    let (ox0, oy0) = polar(r_out, a0);
    let (ox1, oy1) = polar(r_out, a1 + eps);
    let (ix1, iy1) = polar(r_in, a1 + eps);
    let (ix0, iy0) = polar(r_in, a0);
    // Must be `> 180`, NOT `% 360 > 180`: a full circle (sweep 360, the
    // single-category donut) needs the large-arc flag SET — its eps-shortened
    // 359.999° arc spans nearly the whole ring; `360 % 360 → 0` cleared it and
    // the ring rendered as an invisible sliver. (Same fix as the frontend
    // `donutSegmentPath` in lib/timesheet.ts.)
    let large = if sweep > 180.0 { 1 } else { 0 };
    format!(
        "M {ox0:.2} {oy0:.2} A {r_out} {r_out} 0 {large} 1 {ox1:.2} {oy1:.2} L {ix1:.2} {iy1:.2} A {r_in} {r_in} 0 {large} 0 {ix0:.2} {iy0:.2} Z"
    )
}

fn donut_svg(buckets: &[(String, i64)]) -> String {
    let top: Vec<(String, i64)> = buckets.iter().take(8).cloned().collect();
    let total: i64 = top.iter().map(|b| b.1).sum();
    if total == 0 {
        return "<p class=\"rp-empty\">Keine aktive Zeit.</p>".into();
    }
    let mut acc = 0i64;
    let mut paths = String::new();
    let mut legend = String::new();
    for (key, secs) in top.iter() {
        let a0 = acc as f64 / total as f64 * 360.0;
        acc += secs;
        let a1 = acc as f64 / total as f64 * 360.0;
        let color = rs::series_color(key);
        paths.push_str(&format!(
            "<path d=\"{}\" fill=\"{color}\"/>",
            donut_path(60.0, 60.0, 55.0, 33.0, a0, a1)
        ));
        legend.push_str(&format!(
            "<div class=row><i style=\"background:{color}\"></i><span class=k>{}</span><span class=v>{}</span></div>",
            esc(key),
            fmt_dur(*secs)
        ));
    }
    format!(
        "<div class=\"ts-donut\"><svg viewBox=\"0 0 120 120\" width=132 height=132>{paths}</svg><div class=lg>{legend}</div></div>"
    )
}

fn bars_svg(buckets: &[(String, i64)], total: i64) -> String {
    if buckets.is_empty() {
        return "<p class=\"rp-empty\">Keine Daten.</p>".into();
    }
    let rows: String = buckets
        .iter()
        .take(8)
        .map(|(key, secs)| {
            let share = if total > 0 { *secs as f64 / total as f64 } else { 0.0 };
            format!(
                "<tr><td>{}</td><td class=\"rp-num\">{}</td><td class=\"rp-num rp-dim\">{:.0} %</td></tr>",
                rs::name_cell(rs::series_color(key), &esc(key), share),
                fmt_dur(*secs),
                share * 100.0
            )
        })
        .collect();
    format!("<table><tbody>{rows}</tbody></table>")
}

/// Build the self-contained HTML report for `events` over `[from, to)`.
/// `claude_tokens` maps project → (tokens_in, tokens_out) for the same range.
pub fn html(
    events: &[TrackEvent],
    claude_tokens: &HashMap<String, (i64, i64)>,
    from: i64,
    to: i64,
    now: i64,
    project_days: &[(String, Vec<crate::tracking::slots::ProjectTotal>)],
) -> String {
    let (mut active, mut idle) = (0i64, 0i64);
    let mut by_app: HashMap<String, i64> = HashMap::new();
    let mut by_host: HashMap<String, i64> = HashMap::new();
    let mut by_cat: HashMap<String, i64> = HashMap::new();
    let mut by_day: HashMap<String, i64> = HashMap::new();
    let mut claude_secs: HashMap<String, i64> = HashMap::new();
    // Per app → (total seconds, source, detail label → (seconds, count)).
    type DetailStats = HashMap<String, (i64, i64)>;
    let mut apps_detail: HashMap<String, (i64, String, DetailStats)> = HashMap::new();
    for e in events {
        let d = effective_dur_s(e, now);
        if d == 0 {
            continue;
        }
        if e.source == "claude" {
            *claude_secs
                .entry(e.project.clone().unwrap_or_else(|| "(unknown)".into()))
                .or_default() += d;
            continue;
        }
        if e.is_idle {
            idle += d;
        } else {
            active += d;
            *by_app.entry(e.app_name.clone()).or_default() += d;
            *by_day.entry(local_date(e.started_at)).or_default() += d;
            if let Some(h) = &e.host {
                *by_host.entry(h.clone()).or_default() += d;
            }
            *by_cat
                .entry(e.category.clone().unwrap_or_else(|| "Uncategorized".into()))
                .or_default() += d;
            let label = if e.source == "browser" {
                e.host
                    .clone()
                    .or_else(|| e.window_title.clone())
                    .unwrap_or_else(|| "(unknown)".into())
            } else {
                e.window_title.clone().unwrap_or_else(|| "(no title)".into())
            };
            let group = apps_detail
                .entry(e.app_name.clone())
                .or_insert_with(|| (0, e.source.clone(), HashMap::new()));
            group.0 += d;
            let det = group.2.entry(label).or_insert((0, 0));
            det.0 += d;
            det.1 += 1;
        }
    }
    let apps = buckets_sorted(by_app);
    let hosts = buckets_sorted(by_host);
    let cats = buckets_sorted(by_cat);
    let mut days: Vec<(String, i64)> = by_day.into_iter().collect();
    days.sort_by(|a, b| a.0.cmp(&b.0));

    let top3 = apps
        .iter()
        .take(3)
        .map(|(k, s)| format!("{} ({})", esc(k), fmt_dur(*s)))
        .collect::<Vec<_>>()
        .join(" · ");
    let range = if local_date(from) == local_date(to.saturating_sub(1)) {
        local_date(from)
    } else {
        format!("{} – {}", local_date(from), local_date(to.saturating_sub(1)))
    };

    let daily_bars = bars_svg(&days, active);
    let app_donut = donut_svg(&apps);
    let host_bars = bars_svg(&hosts, active);
    let cat_bars = bars_svg(&cats, active);

    // Claude-Code usage per project (time + tokens) — its own section.
    let claude_card = if claude_secs.is_empty() {
        String::new()
    } else {
        let mut rows: Vec<(String, i64)> = claude_secs.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut body = String::from(
            "<table><thead><tr><th>Projekt</th><th class=\"rp-num\">Zeit</th><th class=\"rp-num\">Token ein</th><th class=\"rp-num\">Token aus</th></tr></thead><tbody>",
        );
        for (proj, secs) in &rows {
            let (tin, tout) = claude_tokens.get(proj).copied().unwrap_or((0, 0));
            body.push_str(&format!(
                "<tr><td>{}</td><td class=\"rp-num\">{}</td><td class=\"rp-num\">{}</td><td class=\"rp-num\">{}</td></tr>",
                esc(proj),
                fmt_dur(*secs),
                tin,
                tout
            ));
        }
        body.push_str("</tbody></table>");
        format!("<section><h2>Claude Code</h2>{body}</section>")
    };

    // By app (detailed) — one collapsible <details> per app (native expand, no
    // JS, so it stays self-contained). Browsers list visited hosts; other apps
    // list window titles.
    let browser_card = if apps_detail.is_empty() {
        String::new()
    } else {
        let mut apps_h: Vec<_> = apps_detail.into_iter().collect();
        apps_h.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        let mut blocks = String::new();
        for (app, (total, source, details)) in &apps_h {
            let mut det: Vec<(&String, &(i64, i64))> = details.iter().collect();
            det.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
            let col = if source == "browser" { "Site" } else { "Window" };
            let mut rows = format!(
                "<table><thead><tr><th>{col}</th><th class=\"rp-num\">Anzahl</th><th class=\"rp-num\">Zeit</th></tr></thead><tbody>"
            );
            for (label, (secs, count)) in &det {
                rows.push_str(&format!(
                    "<tr><td>{}</td><td class=\"rp-num\">{}</td><td class=\"rp-num\">{}</td></tr>",
                    esc(label),
                    count,
                    fmt_dur(*secs)
                ));
            }
            rows.push_str("</tbody></table>");
            blocks.push_str(&format!(
                "<details><summary><span class=n>{}</span><span class=dur>{}</span></summary>{}</details>",
                esc(app),
                fmt_dur(*total),
                rows
            ));
        }
        format!("<section><h2>Nach App (im Detail)</h2>{blocks}</section>")
    };

    let stats_strip = rs::stats(&[
        rs::Stat { label: "Aktiv", value: fmt_dur(active), unit: None },
        rs::Stat { label: "Leerlauf", value: fmt_dur(idle), unit: None },
        rs::Stat { label: "Ereignisse", value: events.len().to_string(), unit: None },
    ]);
    let body = format!(
        r#"{stats_strip}
{slots_card}
<section><h2>Aktive Zeit je Tag</h2>{daily_bars}</section>
<div class="ts-cols">
  <section><h2>Nach App</h2>{app_donut}</section>
  <section><h2>Nach Kategorie</h2>{cat_bars}</section>
</div>
<section><h2>Häufigste Hosts</h2>{host_bars}</section>
{browser_card}
{claude_card}
<section><h2>Ereignisse</h2>{table}</section>"#,
        stats_strip = stats_strip,
        daily_bars = daily_bars,
        app_donut = app_donut,
        cat_bars = cat_bars,
        host_bars = host_bars,
        browser_card = browser_card,
        claude_card = claude_card,
        slots_card = project_section_html(project_days),
        table = events_table(events, now),
    );
    let subject = format!(
        "{} · Top: {}",
        esc(&range),
        if top3.is_empty() { "—".into() } else { top3 }
    );
    rs::shell("Zeiterfassung", "Timesheet", &subject, &body, FOOTER)
        .replace("</style>", &format!("{EXTRA_CSS}\n</style>"))
}

/// The consolidated **per-project** section for the HTML report: one row per
/// project per day (date · project · overlap-corrected hours · first–last ·
/// apps). Empty string when nothing consolidated, so it silently vanishes.
fn project_section_html(project_days: &[(String, Vec<crate::tracking::slots::ProjectTotal>)]) -> String {
    let total: usize = project_days.iter().map(|(_, p)| p.len()).sum();
    if total == 0 {
        return String::new();
    }
    let mut rows = String::new();
    let mut grand = 0i64;
    for (date, projects) in project_days {
        for p in projects {
            grand += p.seconds;
            let apps = p.apps.iter().take(3).map(|a| esc(&a.app)).collect::<Vec<_>>().join(", ");
            rows.push_str(&format!(
                "<tr><td>{}</td><td class=\"rp-text\">{}</td><td class=\"rp-num\">{}</td><td class=\"rp-text\">{}–{}</td><td class=\"rp-text\">{}</td></tr>",
                esc(date),
                esc(&p.project),
                hours2(p.seconds),
                local_time(p.start_ms),
                local_time(p.end_ms),
                apps,
            ));
        }
    }
    format!(
        "<section><h2>Konsolidiert je Projekt</h2>\
         <p class=\"rp-lede\">Überlappungsbereinigte Vereinigung je Projekt — richtig auch bei \
         parallelen Sitzungen. {n} Einträge · {h} h gesamt.</p>\
         <table><thead><tr><th>Datum</th><th class=\"rp-text\">Projekt</th><th class=\"rp-num\">Stunden</th>\
         <th class=\"rp-text\">Erster–Letzter</th><th class=\"rp-text\">Apps</th></tr></thead><tbody>{rows}</tbody></table></section>",
        n = total,
        h = hours2(grand),
        rows = rows,
    )
}

/// Decimal hours with two places (`9000` s → `2.50`).
fn hours2(secs: i64) -> String {
    format!("{:.2}", secs as f64 / 3600.0)
}

fn events_table(events: &[TrackEvent], now: i64) -> String {
    let mut t = String::from(
        "<table><thead><tr><th>Datum</th><th class=\"rp-text\">Beginn</th><th class=\"rp-text\">Ende</th><th class=\"rp-text\">App</th><th class=\"rp-text\">Host / Titel</th><th class=\"rp-text\">Quelle</th><th class=\"rp-num\">Dauer</th></tr></thead><tbody>",
    );
    for e in events {
        t.push_str(&format!(
            "<tr><td>{}</td><td class=\"rp-text\">{}</td><td class=\"rp-text\">{}</td><td class=\"rp-text\">{}</td><td class=\"rp-text\">{}</td><td class=\"rp-text ts-src\">{}</td><td class=\"rp-num\">{}</td></tr>",
            local_date(e.started_at),
            local_time(e.started_at),
            e.ended_at.map(local_time).unwrap_or_default(),
            esc(&e.app_name),
            esc(e.host.as_deref().or(e.window_title.as_deref()).unwrap_or("")),
            if e.is_idle { "idle".into() } else { esc(&e.source) },
            fmt_dur(effective_dur_s(e, now)),
        ));
    }
    t.push_str("</tbody></table>");
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(app: &str, start: i64, end: i64, idle: bool, host: Option<&str>, title: Option<&str>) -> TrackEvent {
        TrackEvent {
            id: 0,
            session_id: 1,
            app_name: app.into(),
            app_id: None,
            window_title: title.map(|t| t.into()),
            url: None,
            host: host.map(|h| h.into()),
            category: Some("Dev".into()),
            project: None,
            source: "focus".into(),
            is_idle: idle,
            started_at: start,
            ended_at: Some(end),
            duration_s: Some((end - start) / 1000),
        }
    }

    fn test_total(project: &str, seconds: i64, start_ms: i64, end_ms: i64) -> crate::tracking::slots::ProjectTotal {
        crate::tracking::slots::ProjectTotal {
            project: project.into(),
            seconds,
            start_ms,
            end_ms,
            apps: vec![crate::tracking::slots::SlotApp { app: "Claude Code".into(), seconds }],
            description: String::new(),
        }
    }

    /// Offline sight check: writes both timesheet documents so they can be
    /// opened and LOOKED at. Test-green and looks-right are different claims —
    /// the `loc` PNG-height defect passed every test.
    ///
    /// `cargo test -p inspector-rust-core --lib export::tests::dump -- --ignored`
    #[test]
    #[ignore]
    fn dump_both_documents_for_a_sight_check() {
        let base = 1_735_722_000_000i64; // 2025-01-01 10:00 local-ish
        let h = 3_600_000i64;
        let events = vec![
            ev("Claude Code", base, base + 2 * h, false, None, Some("inspector-rust — device_sync.rs")),
            ev("Ghostty", base + 2 * h, base + 2 * h + h / 2, false, None, Some("cargo test")),
            ev("Safari", base + 3 * h, base + 3 * h + h / 3, false, Some("docs.rs"), Some("rusqlite — Rust")),
            ev("Slack", base + 4 * h, base + 4 * h + h / 6, false, None, Some("#team")),
            ev("Idle", base + 5 * h, base + 5 * h + h, true, None, None),
        ];
        let totals = vec![(
            "2025-01-01".to_string(),
            vec![
                test_total("inspector-rust", 9000, base, base + 2 * h + h / 2),
                test_total("celox-portal", 3600, base + 3 * h, base + 4 * h),
            ],
        )];
        let mut events = events;
        events[0].project = Some("inspector-rust".into());
        events[1].project = Some("inspector-rust".into());
        events[2].project = Some("celox-portal".into());
        let events = events;
        let mut tokens = HashMap::new();
        tokens.insert("inspector-rust".to_string(), (412_000i64, 38_000i64));
        let now = base + 6 * h;

        let dir = std::path::PathBuf::from(
            std::env::var("IR_DUMP_DIR").unwrap_or_else(|_| "/tmp".into()),
        );
        let a = html(&events, &tokens, base, now, now, &totals);
        let b = project_html(&events, base, now, now, Detail::Summary, None);
        std::fs::write(dir.join("timesheet-report.html"), &a).unwrap();
        std::fs::write(dir.join("timesheet-projects.html"), &b).unwrap();
        eprintln!("wrote {} + {} bytes to {}", a.len(), b.len(), dir.display());
    }

    #[test]
    fn both_documents_are_light_and_print_ready() {
        // ⚠️ These two used to be DARK — wrong artefact for something you
        // print or hand a client, and out of step with every other report.
        // Also pins the print rule the PDF path depends on: without
        // print-color-adjust WebKit drops every coloured bar.
        for doc in [
            html(&[], &Default::default(), 0, 86_400_000, 0, &[]),
            project_html(&[], 0, 86_400_000, 0, Detail::Summary, None),
        ] {
            // ⚠️ Nicht mehr die Schreibweise einer Kopie pinnen — das tat die
            // erste Fassung und war grün, WÄHREND beide Dokumente ein eigenes,
            // handgeschriebenes Stylesheet trugen. Jetzt wird geprüft, dass
            // wirklich das GETEILTE benutzt wird; dessen eigene Tests pinnen
            // hell, A4 und print-color-adjust an einer Stelle.
            assert!(
                doc.contains(&crate::report_style::css()),
                "Dokument muss das geteilte Stylesheet einbetten"
            );
            assert!(doc.contains("rp-kicker"), "gemeinsamer Kopf fehlt");
            assert!(!doc.contains("#0c0d11"), "dunkle Palette darf nicht zurückkehren");
        }
    }

    #[test]
    fn csv_appends_consolidated_project_section() {
        let events = vec![ev("Code", 0, 60_000, false, None, None)];
        let totals = vec![("2026-07-21".to_string(), vec![test_total("kiez-finder", 9_000, 0, 9_000_000)])];
        let out = csv(&events, 0, &totals);
        // The consolidated summary must come FIRST — before the raw events —
        // so it isn't buried under hundreds of rows.
        assert!(out.starts_with("# Consolidated per project"));
        assert!(out.contains("date,project,hours,first,last,apps"));
        assert!(out.contains("kiez-finder"));
        assert!(out.contains("2.50")); // 9000 s → 2.50 h
        assert!(out.find("# Consolidated per project").unwrap() < out.find("# Raw events").unwrap());
        // Nothing consolidated → no section, just the raw block.
        assert!(!csv(&events, 0, &[]).contains("Consolidated per project"));
    }

    #[test]
    fn html_includes_consolidated_project_section() {
        let events = vec![ev("Code", 0, 60_000, false, None, None)];
        let totals = vec![("2026-07-21".to_string(), vec![test_total("kiez-finder", 9_000, 0, 9_000_000)])];
        let out = html(&events, &HashMap::new(), 0, 86_400_000, 100_000, &totals);
        assert!(out.contains("Konsolidiert je Projekt"));
        assert!(out.contains("kiez-finder"));
        assert!(out.contains("2.50"));
        // Empty report omits the section entirely.
        assert!(!html(&events, &HashMap::new(), 0, 86_400_000, 100_000, &[]).contains("Consolidated per project"));
    }

    #[test]
    fn csv_consolidated_block_spans_multiple_days_and_leads_the_file() {
        // A week export: two days, each with a project row. Both dates + both
        // projects appear in the consolidated block, and the whole block still
        // precedes the raw-events section.
        let events = vec![ev("Code", 0, 60_000, false, None, None)];
        let totals = vec![
            ("2026-07-21".to_string(), vec![test_total("alpha", 3_600, 0, 3_600_000)]),
            ("2026-07-22".to_string(), vec![test_total("beta", 1_800, 0, 1_800_000)]),
        ];
        let out = csv(&events, 0, &totals);
        let cons = out.find("# Consolidated per project").unwrap();
        let raw = out.find("# Raw events").unwrap();
        assert!(cons < raw);
        assert!(out.contains("2026-07-21,alpha,1.00"));
        assert!(out.contains("2026-07-22,beta,0.50"));
    }

    #[test]
    fn html_project_section_shows_the_grand_total() {
        // Two projects (1.00 h + 0.50 h) → the card badge sums them to 1.50 h.
        let events = vec![ev("Code", 0, 60_000, false, None, None)];
        let totals = vec![(
            "2026-07-22".to_string(),
            vec![
                test_total("alpha", 3_600, 0, 3_600_000),
                test_total("beta", 1_800, 0, 1_800_000),
            ],
        )];
        let out = html(&events, &HashMap::new(), 0, 86_400_000, 100_000, &totals);
        assert!(out.contains("2 Einträge · 1.50 h")); // Anzahl + Gesamtsumme
    }

    #[test]
    fn csv_has_header_and_escapes() {
        let events = vec![ev("Code, Inc", 0, 60_000, false, Some("github.com"), Some("a \"b\""))];
        let out = csv(&events, 0, &[]);
        // With no consolidation, the file starts straight with the raw-events
        // section (its `# Raw events` header + column row).
        assert!(out.starts_with("# Raw events\ndate,start,end,duration_min,app,category,project,host,title,source,idle\n"));
        assert!(out.contains("\"Code, Inc\"")); // comma → quoted
        assert!(out.contains("\"a \"\"b\"\"\"")); // quotes doubled
        assert!(out.contains("github.com"));
        assert!(out.contains(",1.0,")); // 60s → 1.0 min
    }

    #[test]
    fn html_is_self_contained_with_footer_and_no_external_requests() {
        let events = vec![
            ev("Code", 0, 600_000, false, None, Some("main.rs")),
            ev("Safari", 600_000, 900_000, false, Some("github.com"), None),
            ev("Code", 900_000, 1_500_000, true, None, None),
        ];
        let out = html(&events, &HashMap::new(), 0, 86_400_000, 2_000_000, &[]);
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains(FOOTER));
        // No external resource references (offline self-contained).
        assert!(!out.contains("http://"));
        assert!(!out.contains("https://"));
        assert!(!out.contains("src=\"http"));
        // Charts present.
        assert!(out.contains("<svg")); // donut
        assert!(out.contains("rp-track")); // Anteils-Spur in der Zeile (loc-Form)
        // Aggregations rendered (Code is top app).
        assert!(out.contains("Code"));
        assert!(out.contains("github.com"));
    }

    #[test]
    fn html_handles_empty() {
        let out = html(&[], &HashMap::new(), 0, 86_400_000, 0, &[]);
        assert!(out.contains(FOOTER));
        assert!(out.contains("Keine aktive Zeit."));
    }

    #[test]
    fn project_export_groups_billable_excludes_idle_and_claude() {
        let mut a = ev("Code", 0, 3_600_000, false, None, Some("feature.rs")); // 1h, Acme
        a.project = Some("Acme".into());
        let mut b = ev("Safari", 3_600_000, 5_400_000, false, Some("docs.rs"), None); // 30m, Acme
        b.project = Some("Acme".into());
        let mut c = ev("Code", 5_400_000, 9_000_000, false, None, Some("x.rs")); // 1h, Beta
        c.project = Some("Beta".into());
        let mut idle = ev("Code", 0, 600_000, true, None, None); // idle, has project → excluded
        idle.project = Some("Acme".into());
        let mut cl = ev("Claude Code", 0, 600_000, false, None, None); // claude → excluded
        cl.source = "claude".into();
        cl.project = Some("Acme".into());
        let untagged = ev("Finder", 0, 600_000, false, None, None); // no project → excluded
        let events = vec![a, b, c, idle, cl, untagged];

        // Full CSV (all projects): one row per entry.
        let csv = project_csv(&events, 0, Detail::Full, None);
        assert!(csv.starts_with("project,date,start,end,duration_min,app,activity\n"));
        assert!(csv.contains("Acme,"));
        assert!(csv.contains("Beta,"));
        assert!(!csv.contains("Finder")); // untagged excluded
        assert!(!csv.contains("Claude Code")); // claude excluded
        assert_eq!(csv.trim().lines().count(), 4); // header + 3 entries

        // Summary CSV: one row per project.
        let sum = project_csv(&events, 0, Detail::Summary, None);
        assert!(sum.starts_with("project,duration_min\n"));
        assert_eq!(sum.trim().lines().count(), 3); // header + 2 projects

        // Daily CSV scoped to one project: only that project's days.
        let acme = project_csv(&events, 0, Detail::Daily, Some("Acme"));
        assert!(acme.starts_with("project,date,duration_min\n"));
        assert!(acme.contains("Acme,"));
        assert!(!acme.contains("Beta")); // scoped out → client B not exposed
        assert_eq!(acme.trim().lines().count(), 2); // header + 1 day

        let html = project_html(&events, 0, 86_400_000, 0, Detail::Full, None);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("Projekt-Report"));
        assert!(html.contains("Acme"));
        assert!(html.contains("Beta"));
        assert!(html.contains("Gesamt"));
        assert!(html.contains(FOOTER));
        assert!(!html.contains("http://"));
        // Scoped HTML excludes the other client entirely.
        let scoped = project_html(&events, 0, 86_400_000, 0, Detail::Summary, Some("Acme"));
        assert!(scoped.contains("Acme"));
        assert!(!scoped.contains("Beta"));
    }

    #[test]
    fn html_app_details_are_collapsible_for_all_apps() {
        let mut chrome = ev("Google Chrome", 0, 120_000, false, Some("github.com"), Some("PR"));
        chrome.source = "browser".into();
        let code = ev("Code", 120_000, 300_000, false, None, Some("main.rs"));
        let out = html(&[chrome, code], &HashMap::new(), 0, 86_400_000, 400_000, &[]);
        assert!(out.contains("Nach App (im Detail)"));
        assert!(out.contains("<details>")); // native expand, no JS
        // Browser → host detail; other app → window-title detail.
        assert!(out.contains("Google Chrome"));
        assert!(out.contains("github.com"));
        assert!(out.contains("main.rs"));
        assert!(!out.contains("http://")); // still self-contained
    }

    /// Regression: a FULL-CIRCLE donut segment (one single category = sweep
    /// 360°) must carry the SVG large-arc flag — `sweep % 360 > 180` computed
    /// `0` for it and the ring rendered as an invisible sliver. (Same bug
    /// existed in the frontend `donutSegmentPath`; both fixed together.)
    #[test]
    fn donut_full_circle_sets_the_large_arc_flag() {
        let full = donut_path(80.0, 80.0, 70.0, 44.0, 0.0, 360.0);
        // Both arc commands ("A rx ry rot LARGE sweep x y") must have large=1.
        assert!(full.contains("A 70 70 0 1 1"), "outer arc must be large: {full}");
        assert!(full.contains("A 44 44 0 1 0"), "inner arc must be large: {full}");
        // A minor segment keeps large=0…
        let minor = donut_path(80.0, 80.0, 70.0, 44.0, 0.0, 90.0);
        assert!(minor.contains("A 70 70 0 0 1"), "minor arc must be small: {minor}");
        // …and a major (>180°) one sets it.
        let major = donut_path(80.0, 80.0, 70.0, 44.0, 0.0, 270.0);
        assert!(major.contains("A 70 70 0 1 1"), "major arc must be large: {major}");
    }
}
