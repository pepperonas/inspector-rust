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
pub fn csv(events: &[TrackEvent], now: i64) -> String {
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
    out
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
    let large = if sweep % 360.0 > 180.0 { 1 } else { 0 };
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
) -> String {
    let (mut active, mut idle) = (0i64, 0i64);
    let mut by_app: HashMap<String, i64> = HashMap::new();
    let mut by_host: HashMap<String, i64> = HashMap::new();
    let mut by_cat: HashMap<String, i64> = HashMap::new();
    let mut by_day: HashMap<String, i64> = HashMap::new();
    let mut claude_secs: HashMap<String, i64> = HashMap::new();
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
{claude_card}
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
        claude_card = claude_card,
        table = events_table(events, now),
        footer = FOOTER,
    )
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

    #[test]
    fn csv_has_header_and_escapes() {
        let events = vec![ev("Code, Inc", 0, 60_000, false, Some("github.com"), Some("a \"b\""))];
        let out = csv(&events, 0);
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
        let out = html(&events, &HashMap::new(), 0, 86_400_000, 2_000_000);
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
        let out = html(&[], &HashMap::new(), 0, 86_400_000, 0);
        assert!(out.contains(FOOTER));
        assert!(out.contains("No active time."));
    }
}
