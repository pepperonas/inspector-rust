//! `pagespeed` report → one self-contained HTML document (v0.142.0).
//!
//! Same contract as [`crate::loc_export`], deliberately: ONE renderer feeds
//! HTML and PDF, the document is self-contained (inline CSS, no script, no
//! external request) and deterministic, and every value is escaped.
//!
//! **Desktop and mobile appear side by side in one document**, never as two
//! files — a page is routinely fine on one and poor on the other, and a
//! report that shows only half of that invites the wrong conclusion.

use crate::pagespeed::{band, PageSpeedReport, StrategyRun};

use crate::loc_export::esc;
use crate::report_style as rs;

/// The ring colour per band — one rule, shared by panel, HTML and PDF.
pub fn band_color(b: &str) -> &'static str {
    match b {
        "good" => "#0cce6b",
        "average" => "#ffa400",
        "poor" => "#ff4e42",
        _ => "#9aa1ab",
    }
}

/// A score as an SVG ring — Lighthouse's own visual, and inline SVG so it
/// survives the PDF (a canvas would not).
fn ring(c: &crate::pagespeed::CategoryScore) -> String {
    let col = band_color(band(c.score));
    let (r, circ) = (25.0_f64, 2.0 * std::f64::consts::PI * 25.0);
    let filled = c.score.map(|s| s as f64 / 100.0).unwrap_or(0.0) * circ;
    let text = c.score.map(|s| s.to_string()).unwrap_or_else(|| "–".into());
    format!(
        r#"<div class="ps-ring"><svg viewBox="0 0 60 60" width="56" height="56" aria-hidden="true">
<circle cx="30" cy="30" r="{r}" fill="none" stroke="{col}" stroke-opacity=".16" stroke-width="5"/>
<circle cx="30" cy="30" r="{r}" fill="none" stroke="{col}" stroke-width="5" stroke-linecap="round"
 stroke-dasharray="{filled:.2} {circ:.2}" transform="rotate(-90 30 30)"/>
<text x="30" y="36" text-anchor="middle" font-size="17" font-weight="640" fill="{col}"
 font-family="-apple-system,BlinkMacSystemFont,Helvetica,Arial,sans-serif">{text}</text>
</svg><span>{label}</span></div>"#,
        label = esc(&c.label),
    )
}

fn strategy_block(run: &StrategyRun, heading: &str) -> String {
    let rings: String = run.categories.iter().map(ring).collect();
    let metrics: String = run
        .metrics
        .iter()
        .map(|m| {
            format!(
                r#"<tr><td><span class="ps-dot" style="background:{c}"></span>{label}</td><td class="rp-num">{val}</td></tr>"#,
                c = band_color(band(m.score)),
                label = esc(&m.label),
                val = esc(&m.display),
            )
        })
        .collect();
    format!(
        r#"<div class="ps-col"><h2>{heading}</h2><div class="ps-rings">{rings}</div>
<table><tbody>{metrics}</tbody></table>
<p class="ps-meta">{url}<br>{time} · Lighthouse {ver}</p></div>"#,
        heading = esc(heading),
        url = esc(&run.final_url),
        time = esc(&fmt_time(&run.fetch_time)),
        ver = esc(&run.lighthouse_version),
    )
}

/// `2026-08-27T20:23:50.193Z` → `27.08.2026, 20:23 UTC`. Pure; an
/// unrecognised string is passed through rather than mangled.
pub fn fmt_time(iso: &str) -> String {
    let (date, rest) = match iso.split_once('T') {
        Some(v) => v,
        None => return iso.to_string(),
    };
    let d: Vec<&str> = date.split('-').collect();
    if d.len() != 3 || rest.len() < 5 {
        return iso.to_string();
    }
    format!("{}.{}.{}, {} UTC", d[2], d[1], d[0], &rest[..5])
}

/// The extra rules this report needs on top of the shared stylesheet: the
/// two strategies sit SIDE BY SIDE so the comparison is a glance, not a
/// scroll — that is the entire reason both are in one document.
fn extra_css() -> &'static str {
    r#"
.ps-cols { display: flex; gap: 34px; align-items: flex-start }
.ps-col { flex: 1 1 0; min-width: 0 }
/* A fixed four-column grid, not flex-wrap: the four categories must sit in
   ONE row per strategy so desktop and mobile line up score-for-score. With
   wrapping, SEO dropped onto a second line and the comparison broke. */
.ps-rings { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; margin: 0 0 18px }
.ps-ring { text-align: center; min-width: 0 }
.ps-ring span { display: block; margin-top: 3px; font-size: 9.5px; line-height: 1.2; color: var(--muted); overflow-wrap: anywhere }
.ps-dot { display: inline-block; width: 8px; height: 8px; border-radius: 2px; margin-right: 9px }
.ps-meta { margin: 12px 0 0; color: var(--muted); font-size: 10.5px; line-height: 1.5; word-break: break-all }
.ps-scale { display: flex; flex-wrap: wrap; gap: 4px 18px; margin: 0 0 26px; font-size: 11.5px; color: var(--muted) }
.ps-scale span { display: inline-flex; align-items: center; gap: 6px }
.ps-scale i { width: 8px; height: 8px; border-radius: 2px }
@media print { .ps-cols { gap: 22px } }
"#
}

/// Build the document. Pure — no clock, so HTML and PDF cannot disagree.
pub fn build_html(r: &PageSpeedReport) -> String {
    let mut cols = String::new();
    if let Some(d) = &r.desktop {
        cols.push_str(&strategy_block(d, "Desktop"));
    }
    if let Some(m) = &r.mobile {
        cols.push_str(&strategy_block(m, "Mobil"));
    }

    let errors: String = if r.errors.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="rp-note">{}</p>"#,
            r.errors.iter().map(|e| esc(e)).collect::<Vec<_>>().join("<br>")
        )
    };

    let body = if cols.is_empty() {
        format!(r#"{errors}<p class="rp-empty">Keine Messung zustande gekommen.</p>"#)
    } else {
        format!(
            r#"{errors}<div class="ps-scale">
<span><i style="background:{good}"></i>90–100 gut</span>
<span><i style="background:{avg}"></i>50–89 verbesserungswürdig</span>
<span><i style="background:{poor}"></i>0–49 schlecht</span>
</div><section class="ps-cols">{cols}</section>"#,
            good = band_color("good"),
            avg = band_color("average"),
            poor = band_color("poor"),
        )
    };

    let doc = rs::shell(
        "PageSpeed Insights",
        "Performance-Bericht",
        &esc(&r.url),
        &body,
        "Gemessen von Google PageSpeed Insights (Lighthouse) auf Googles Infrastruktur. \
         <b>Desktop</b> und <b>Mobil</b> sind getrennte Läufe; Performance-Werte schwanken \
         zwischen Messungen.<br>Erstellt mit Inspector Rust.",
    );
    // Append this report's own rules to the shared stylesheet.
    doc.replace("</style>", &format!("{}\n</style>", extra_css()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pagespeed::{CategoryScore, Metric};

    fn run(strategy: &str, perf: Option<u8>) -> StrategyRun {
        StrategyRun {
            strategy: strategy.into(),
            categories: vec![
                CategoryScore { id: "performance".into(), label: "Performance".into(), score: perf },
                CategoryScore { id: "seo".into(), label: "SEO".into(), score: Some(100) },
            ],
            metrics: vec![Metric {
                id: "first-contentful-paint".into(),
                label: "First Contentful Paint".into(),
                display: "1.5 s".into(),
                score: Some(93),
            }],
            final_url: "https://celox.io/".into(),
            fetch_time: "2026-08-27T19:44:05.026Z".into(),
            lighthouse_version: "13.4.1".into(),
        }
    }

    fn report() -> PageSpeedReport {
        PageSpeedReport {
            url: "https://celox.io".into(),
            desktop: Some(run("desktop", Some(95))),
            mobile: Some(run("mobile", Some(42))),
            errors: vec![],
        }
    }

    #[test]
    fn both_strategies_land_in_one_document() {
        // The whole point: desktop and mobile together. A page is routinely
        // fine on one and poor on the other.
        let h = build_html(&report());
        assert!(h.contains("Desktop"));
        assert!(h.contains("Mobil"));
        assert_eq!(h.matches(r#"class="ps-col""#).count(), 2);
        // …and they sit side by side, which is the point of one document.
        assert!(h.contains(r#"class="ps-cols""#));
    }

    #[test]
    fn the_four_rings_stay_in_one_row_per_strategy() {
        // ⚠️ With flex-wrap the fourth category (SEO) dropped onto a second
        // line and desktop/mobile no longer lined up score-for-score, which
        // is the only reason both are in one document.
        let h = build_html(&report());
        assert!(h.contains("grid-template-columns: repeat(4, 1fr)"));
        assert!(!h.contains(".ps-rings { display: flex"));
    }

    #[test]
    fn the_document_is_self_contained() {
        let h = build_html(&report());
        for forbidden in ["<script", "http://", "@import", "<link", "src="] {
            assert!(!h.contains(forbidden), "{forbidden} darf nicht vorkommen");
        }
        // The analysed URL is the one https:// that legitimately appears —
        // as text, never as a fetched resource.
        assert!(h.starts_with("<!doctype html>") && h.contains("</html>"));
    }

    #[test]
    fn scores_are_coloured_by_the_shared_band_rule() {
        let h = build_html(&report());
        assert!(h.contains(band_color("good"))); // 95 and 100
        assert!(h.contains(band_color("poor"))); // 42 on mobile
        assert_eq!(band_color("average"), "#ffa400");
        assert_eq!(band_color("unknown"), "#9aa1ab");
    }

    #[test]
    fn an_unscored_category_shows_a_dash_not_a_zero() {
        let mut r = report();
        r.desktop = Some(run("desktop", None));
        let h = build_html(&r);
        assert!(h.contains(">–<"), "Gedankenstrich statt 0 erwartet");
    }

    #[test]
    fn a_half_failed_analysis_states_the_reason() {
        // ⚠️ Never let a missing half read as "this page has no data".
        let mut r = report();
        r.mobile = None;
        r.errors = vec!["Mobil: Kontingent erschöpft".into()];
        let h = build_html(&r);
        assert!(h.contains("Kontingent"));
        assert_eq!(h.matches(r#"class="ps-col""#).count(), 1);
    }

    #[test]
    fn a_completely_failed_analysis_still_renders() {
        let r = PageSpeedReport {
            url: "https://a.de".into(),
            desktop: None,
            mobile: None,
            errors: vec!["Desktop: kaputt".into(), "Mobil: kaputt".into()],
        };
        let h = build_html(&r);
        assert!(h.contains("Keine Messung"));
        assert!(h.contains("Desktop: kaputt") && h.contains("Mobil: kaputt"));
        assert!(h.contains("</html>"));
    }

    #[test]
    fn everything_from_the_network_is_escaped() {
        let mut r = report();
        r.url = "https://a.de/<script>alert(1)</script>".into();
        if let Some(d) = r.desktop.as_mut() {
            d.final_url = "\"><b>x".into();
            d.categories[0].label = "A&B".into();
        }
        let h = build_html(&r);
        assert!(!h.contains("<script>alert"));
        assert!(!h.contains("\"><b>x"));
        assert!(h.contains("A&amp;B"));
    }

    #[test]
    fn it_is_deterministic_so_html_and_pdf_cannot_disagree() {
        assert_eq!(build_html(&report()), build_html(&report()));
    }
}
