//! Timesheet export — flat **CSV** and a **single self-contained HTML** report
//! (CSS + charts inline, zero external requests, dark theme, offline-viewable).
//! Charts are server-rendered inline **SVG** (no JS needed to view). Pure
//! builders over already-decrypted [`TrackEvent`]s so they're unit-testable.

use super::db::TrackEvent;
use chrono::{Local, TimeZone};
use std::collections::HashMap;

const FOOTER: &str = "© 2026 Martin Pfeffer | celox.io";
const PALETTE: [&str; 10] = [
    "#b3c5ff", "#7dd3fc", "#86efac", "#fcd34d", "#f9a8d4", "#c4b5fd", "#fdba74", "#5eead4",
    "#a3e635", "#f87171",
];

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
    slot_days: &[(String, Vec<crate::tracking::slots::Slot>)],
) -> String {
    let mut out =
        String::from("date,start,end,duration_min,app,category,project,host,title,source,idle\n");
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
    out.push_str(&slots_section_csv(slot_days));
    out
}

/// The consolidated slots as a second, clearly delimited CSV block appended
/// after the raw events (so a spreadsheet import sees the header row again).
/// Empty when there are no slots. `hours` is the bookable (snapped) span.
fn slots_section_csv(slot_days: &[(String, Vec<crate::tracking::slots::Slot>)]) -> String {
    if slot_days.iter().all(|(_, s)| s.is_empty()) {
        return String::new();
    }
    let mut out = String::from("\n# Consolidated slots\ndate,start,end,hours,project,description\n");
    for (date, slots) in slot_days {
        for s in slots {
            let row = [
                date.clone(),
                local_time(s.start_ms),
                local_time(s.end_ms),
                hours2(s.span_s),
                s.label.clone(),
                s.description.clone(),
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
        let mut body = String::from(
            "<table><thead><tr><th>Project</th><th class=r>Duration</th></tr></thead><tbody>",
        );
        let mut i = 0;
        while i < billable.len() {
            let proj = billable[i].project.clone().unwrap_or_default();
            let mut total = 0i64;
            while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                total += effective_dur_s(billable[i], now);
                i += 1;
            }
            body.push_str(&format!(
                "<tr><td>{}</td><td class=r>{}</td></tr>",
                esc(&proj),
                fmt_dur(total)
            ));
        }
        body.push_str("</tbody></table>");
        sections = format!("<div class=card>{body}</div>");
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
                        "<tr><td>{}</td><td class=r>{}</td></tr>",
                        date,
                        fmt_dur(*d)
                    ));
                }
                sections.push_str(&format!(
                    "<div class=card><div class=phead><h2>{}</h2><span class=ptot>{}</span></div>\
                     <table><thead><tr><th>Date</th><th class=r>Duration</th></tr></thead><tbody>{}</tbody></table></div>",
                    esc(&proj), fmt_dur(ptotal), rows
                ));
            } else {
                while i < billable.len() && billable[i].project.as_deref() == Some(proj.as_str()) {
                    let e = billable[i];
                    let d = effective_dur_s(e, now);
                    ptotal += d;
                    rows.push_str(&format!(
                        "<tr><td>{}</td><td>{}–{}</td><td class=r>{}</td><td>{}</td></tr>",
                        local_date(e.started_at),
                        local_time(e.started_at),
                        e.ended_at.map(local_time).unwrap_or_default(),
                        fmt_dur(d),
                        esc(&activity(e)),
                    ));
                    i += 1;
                }
                sections.push_str(&format!(
                    "<div class=card><div class=phead><h2>{}</h2><span class=ptot>{}</span></div>\
                     <table><thead><tr><th>Date</th><th>Time</th><th class=r>Duration</th><th>Activity</th></tr></thead><tbody>{}</tbody></table></div>",
                    esc(&proj), fmt_dur(ptotal), rows
                ));
            }
        }
    }
    if billable.is_empty() {
        sections = "<p class=muted>No project-tagged time in this range. Assign time to a project by dragging a window on the day timeline.</p>".into();
    }

    format!(
        r#"<!doctype html><html lang=en><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>Project report · {range}</title>
<style>
:root{{--bg:#0c0d11;--surface:#17191f;--border:#2b2e38;--muted:#9a9fac;--fg:#f2f3f5;--accent:#b3c5ff}}
*{{box-sizing:border-box}}
body{{margin:0 auto;max-width:880px;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;padding:28px}}
h1{{font-size:22px;margin:0 0 4px}}
.sub{{color:var(--muted);margin:0 0 20px}}
.card{{border:1px solid var(--border);border-radius:14px;padding:14px;margin-bottom:16px}}
.phead{{display:flex;justify-content:space-between;align-items:baseline;margin-bottom:8px}}
.card h2{{font-size:15px;margin:0}}
.ptot{{font-weight:700;color:var(--accent);font-variant-numeric:tabular-nums}}
.muted{{color:var(--muted)}}
table{{width:100%;border-collapse:collapse;font-size:12px}}
th,td{{text-align:left;padding:5px 6px;border-bottom:1px solid var(--border)}}
th{{color:var(--muted);font-weight:500}}
td.r,th.r{{text-align:right;font-variant-numeric:tabular-nums}}
.grand{{font-size:16px;font-weight:700;text-align:right;margin:4px 2px 0}}
.grand .accent{{color:var(--accent)}}
footer{{color:var(--muted);text-align:center;margin-top:28px;font-size:12px}}
@media print{{body{{padding:0}}.card{{break-inside:avoid}}}}
</style></head><body>
<h1>Project report</h1>
<p class=sub>{range}{scope}</p>
{sections}
<p class=grand>Total: <span class=accent>{grand}</span></p>
<footer>{footer}</footer>
</body></html>"#,
        range = esc(&range),
        scope = scope,
        sections = sections,
        grand = fmt_dur(grand),
        footer = FOOTER,
    )
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
        return "<p class=muted>No active time.</p>".into();
    }
    let mut acc = 0i64;
    let mut paths = String::new();
    let mut legend = String::new();
    for (i, (key, secs)) in top.iter().enumerate() {
        let a0 = acc as f64 / total as f64 * 360.0;
        acc += secs;
        let a1 = acc as f64 / total as f64 * 360.0;
        let color = PALETTE[i % PALETTE.len()];
        paths.push_str(&format!(
            "<path d=\"{}\" fill=\"{color}\"/>",
            donut_path(60.0, 60.0, 55.0, 33.0, a0, a1)
        ));
        legend.push_str(&format!(
            "<div class=row><span class=dot style=\"background:{color}\"></span><span class=k>{}</span><span class=v>{}</span></div>",
            esc(key),
            fmt_dur(*secs)
        ));
    }
    format!(
        "<div class=donut><svg viewBox=\"0 0 120 120\" width=140 height=140>{paths}</svg><div class=legend>{legend}</div></div>"
    )
}

fn bars_svg(buckets: &[(String, i64)], total: i64) -> String {
    if buckets.is_empty() {
        return "<p class=muted>No data.</p>".into();
    }
    let max = buckets.iter().map(|b| b.1).max().unwrap_or(1).max(1);
    let mut out = String::from("<div class=bars>");
    for (key, secs) in buckets.iter().take(8) {
        let pct = (*secs as f64 / max as f64 * 100.0).round();
        let share = if total > 0 {
            format!(" · {}%", (*secs as f64 / total as f64 * 100.0).round())
        } else {
            String::new()
        };
        out.push_str(&format!(
            "<div class=bar><div class=lbl><span>{}</span><span class=v>{}{}</span></div><div class=track><div class=fill style=\"width:{pct}%\"></div></div></div>",
            esc(key),
            fmt_dur(*secs),
            share
        ));
    }
    out.push_str("</div>");
    out
}

/// Build the self-contained HTML report for `events` over `[from, to)`.
/// `claude_tokens` maps project → (tokens_in, tokens_out) for the same range.
pub fn html(
    events: &[TrackEvent],
    claude_tokens: &HashMap<String, (i64, i64)>,
    from: i64,
    to: i64,
    now: i64,
    slot_days: &[(String, Vec<crate::tracking::slots::Slot>)],
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
            "<table><thead><tr><th>Project</th><th class=r>Time</th><th class=r>Tokens in</th><th class=r>Tokens out</th></tr></thead><tbody>",
        );
        for (proj, secs) in &rows {
            let (tin, tout) = claude_tokens.get(proj).copied().unwrap_or((0, 0));
            body.push_str(&format!(
                "<tr><td>{}</td><td class=r>{}</td><td class=r>{}</td><td class=r>{}</td></tr>",
                esc(proj),
                fmt_dur(*secs),
                tin,
                tout
            ));
        }
        body.push_str("</tbody></table>");
        format!("<div class=card><h2>Claude Code</h2>{body}</div>")
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
                "<table><thead><tr><th>{col}</th><th class=r>Count</th><th class=r>Time</th></tr></thead><tbody>"
            );
            for (label, (secs, count)) in &det {
                rows.push_str(&format!(
                    "<tr><td>{}</td><td class=r>{}</td><td class=r>{}</td></tr>",
                    esc(label),
                    count,
                    fmt_dur(*secs)
                ));
            }
            rows.push_str("</tbody></table>");
            blocks.push_str(&format!(
                "<details><summary><span>{}</span><span class=dur>{}</span></summary>{}</details>",
                esc(app),
                fmt_dur(*total),
                rows
            ));
        }
        format!("<div class=card><h2>By app (detailed)</h2>{blocks}</div>")
    };

    format!(
        r#"<!doctype html><html lang=en><head><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>Timesheet · {range}</title>
<style>
:root{{--bg:#0c0d11;--surface:#17191f;--border:#2b2e38;--muted:#9a9fac;--fg:#f2f3f5;--accent:#b3c5ff}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;padding:28px;max-width:980px;margin:0 auto}}
h1{{font-size:22px;margin:0 0 4px}}
.sub{{color:var(--muted);margin:0 0 20px}}
.stats{{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-bottom:20px}}
.stat{{border:1px solid var(--border);border-radius:14px;padding:14px}}
.stat .l{{color:var(--muted);font-size:11px;text-transform:uppercase;letter-spacing:.04em}}
.stat .n{{font-size:24px;font-weight:700;margin-top:2px}}
.stat .n.accent{{color:var(--accent)}}
.card{{border:1px solid var(--border);border-radius:14px;padding:14px;margin-bottom:16px}}
.card h2{{font-size:13px;color:var(--muted);font-weight:500;margin:0 0 10px}}
.muted{{color:var(--muted)}}
.grid2{{display:grid;grid-template-columns:1fr 1fr;gap:16px}}
.donut{{display:flex;gap:14px;align-items:center}}
.legend{{flex:1;min-width:0}}
.legend .row{{display:flex;align-items:center;gap:8px;font-size:12px;margin:2px 0}}
.dot{{width:10px;height:10px;border-radius:50%;flex:none}}
.legend .k{{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}
.legend .v{{color:var(--muted)}}
.bars .bar{{margin:6px 0;font-size:12px}}
.bars .lbl{{display:flex;justify-content:space-between;margin-bottom:3px}}
.bars .v{{color:var(--muted)}}
.track{{height:7px;background:var(--surface);border-radius:99px;overflow:hidden}}
.fill{{height:100%;background:var(--accent);border-radius:99px}}
table{{width:100%;border-collapse:collapse;font-size:12px}}
th,td{{text-align:left;padding:5px 6px;border-bottom:1px solid var(--border)}}
th{{color:var(--muted);font-weight:500}}
td.r,th.r{{text-align:right;font-variant-numeric:tabular-nums}}
.badge{{background:var(--surface);color:var(--muted);border-radius:99px;padding:1px 7px;font-size:10px}}
details{{border:1px solid var(--border);border-radius:10px;margin:6px 0;overflow:hidden}}
details+details{{margin-top:8px}}
summary{{display:flex;justify-content:space-between;align-items:center;cursor:pointer;padding:9px 12px;font-weight:600;list-style:none}}
summary::-webkit-details-marker{{display:none}}
summary::before{{content:"▸";color:var(--muted);margin-right:8px;transition:transform .15s}}
details[open] summary::before{{transform:rotate(90deg)}}
summary span:first-of-type{{flex:1}}
summary .dur{{color:var(--muted);font-variant-numeric:tabular-nums;font-weight:500}}
details>table{{margin:0 12px 10px}}
footer{{color:var(--muted);text-align:center;margin-top:28px;font-size:12px}}
</style></head><body>
<h1>Timesheet</h1>
<p class=sub>{range} · Top: {top3}</p>
<div class=stats>
  <div class=stat><div class=l>Active</div><div class="n accent">{active}</div></div>
  <div class=stat><div class=l>Idle</div><div class=n>{idle}</div></div>
  <div class=stat><div class=l>Events</div><div class=n>{nevents}</div></div>
</div>
<div class=card><h2>Active time per day</h2>{daily_bars}</div>
<div class=grid2>
  <div class=card><h2>By app</h2>{app_donut}</div>
  <div class=card><h2>By category</h2>{cat_bars}</div>
</div>
<div class=card><h2>Top hosts</h2>{host_bars}</div>
{browser_card}
{claude_card}
{slots_card}
<div class=card><h2>Events</h2>{table}</div>
<footer>{footer}</footer>
</body></html>"#,
        range = esc(&range),
        top3 = if top3.is_empty() { "—".into() } else { top3 },
        active = fmt_dur(active),
        idle = fmt_dur(idle),
        nevents = events.len(),
        daily_bars = daily_bars,
        app_donut = app_donut,
        cat_bars = cat_bars,
        host_bars = host_bars,
        browser_card = browser_card,
        claude_card = claude_card,
        slots_card = slots_section_html(slot_days),
        table = events_table(events, now),
        footer = FOOTER,
    )
}

/// The consolidated-slots section for the HTML report: one row per bookable
/// slot (date · time span · project · hours · description). Empty string when
/// there are no slots, so it silently vanishes from a slot-less report.
fn slots_section_html(slot_days: &[(String, Vec<crate::tracking::slots::Slot>)]) -> String {
    let total: usize = slot_days.iter().map(|(_, s)| s.len()).sum();
    if total == 0 {
        return String::new();
    }
    let mut rows = String::new();
    let mut grand = 0i64;
    for (date, slots) in slot_days {
        for s in slots {
            grand += s.span_s;
            rows.push_str(&format!(
                "<tr><td>{}</td><td>{}–{}</td><td>{}</td><td class=r>{}</td><td>{}</td></tr>",
                esc(date),
                local_time(s.start_ms),
                local_time(s.end_ms),
                esc(&s.label),
                hours2(s.span_s),
                esc(&s.description),
            ));
        }
    }
    format!(
        "<div class=card><h2>Consolidated slots \
         <span class=badge>{n} · {h} h</span></h2>\
         <table><thead><tr><th>Date</th><th>Time</th><th>Project</th>\
         <th class=r>Hours</th><th>Description</th></tr></thead><tbody>{rows}</tbody></table></div>",
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
        "<table><thead><tr><th>Date</th><th>Start</th><th>End</th><th>App</th><th>Host / Title</th><th>Src</th><th class=r>Dur</th></tr></thead><tbody>",
    );
    for e in events {
        t.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><span class=badge>{}</span></td><td class=r>{}</td></tr>",
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

    fn test_slot(label: &str, start_ms: i64, end_ms: i64, desc: &str) -> crate::tracking::slots::Slot {
        crate::tracking::slots::Slot {
            start_ms,
            end_ms,
            project: Some(label.into()),
            label: label.into(),
            description: desc.into(),
            origin: crate::tracking::slots::Origin::Tagged,
            apps: vec![],
            event_ids: vec![1],
            active_s: (end_ms - start_ms) / 1000,
            span_s: (end_ms - start_ms) / 1000,
            confidence: 1.0,
        }
    }

    #[test]
    fn csv_appends_consolidated_slots_section() {
        let events = vec![ev("Code", 0, 60_000, false, None, None)];
        let slots = vec![("2026-07-21".to_string(), vec![test_slot("kiez-finder", 0, 9_000_000, "branch work")])];
        let out = csv(&events, 0, &slots);
        assert!(out.contains("# Consolidated slots"));
        assert!(out.contains("date,start,end,hours,project,description"));
        assert!(out.contains("kiez-finder"));
        assert!(out.contains("2.50")); // 9000 s → 2.50 h
        // No slots → no section.
        assert!(!csv(&events, 0, &[]).contains("Consolidated slots"));
    }

    #[test]
    fn html_includes_consolidated_slots_section() {
        let events = vec![ev("Code", 0, 60_000, false, None, None)];
        let slots = vec![("2026-07-21".to_string(), vec![test_slot("kiez-finder", 0, 9_000_000, "branch work")])];
        let out = html(&events, &HashMap::new(), 0, 86_400_000, 100_000, &slots);
        assert!(out.contains("Consolidated slots"));
        assert!(out.contains("kiez-finder"));
        assert!(out.contains("2.50"));
        // Slot-less report omits the section entirely.
        assert!(!html(&events, &HashMap::new(), 0, 86_400_000, 100_000, &[]).contains("Consolidated slots"));
    }

    #[test]
    fn csv_has_header_and_escapes() {
        let events = vec![ev("Code, Inc", 0, 60_000, false, Some("github.com"), Some("a \"b\""))];
        let out = csv(&events, 0, &[]);
        assert!(out.starts_with("date,start,end,duration_min,app,category,project,host,title,source,idle\n"));
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
        assert!(out.contains("class=fill")); // bars
        // Aggregations rendered (Code is top app).
        assert!(out.contains("Code"));
        assert!(out.contains("github.com"));
    }

    #[test]
    fn html_handles_empty() {
        let out = html(&[], &HashMap::new(), 0, 86_400_000, 0, &[]);
        assert!(out.contains(FOOTER));
        assert!(out.contains("No active time."));
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
        assert!(html.contains("Project report"));
        assert!(html.contains("Acme"));
        assert!(html.contains("Beta"));
        assert!(html.contains("Total:"));
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
        assert!(out.contains("By app (detailed)"));
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
