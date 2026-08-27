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

/// The ring colour per band — one rule, shared by panel, HTML and PDF.
pub fn band_color(b: &str) -> &'static str {
    match b {
        "good" => "#0cce6b",
        "average" => "#ffa400",
        "poor" => "#ff4e42",
        _ => "#9aa1ab",
    }
}

/// A score as an SVG ring — Lighthouse's own visual, and it survives the PDF
/// because it is inline SVG rather than a canvas.
fn ring(c: &crate::pagespeed::CategoryScore) -> String {
    let col = band_color(band(c.score));
    let (r, circ) = (26.0_f64, 2.0 * std::f64::consts::PI * 26.0);
    let filled = c.score.map(|s| s as f64 / 100.0).unwrap_or(0.0) * circ;
    let text = c.score.map(|s| s.to_string()).unwrap_or_else(|| "–".into());
    format!(
        r#"<div class="ring"><svg viewBox="0 0 64 64" width="64" height="64" aria-hidden="true">
<circle cx="32" cy="32" r="{r}" fill="none" stroke="{col}" stroke-opacity=".18" stroke-width="6"/>
<circle cx="32" cy="32" r="{r}" fill="none" stroke="{col}" stroke-width="6" stroke-linecap="round"
 stroke-dasharray="{filled:.2} {circ:.2}" transform="rotate(-90 32 32)"/>
<text x="32" y="37" text-anchor="middle" font-size="18" font-weight="600" fill="{col}">{text}</text>
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
                r#"<tr><td><i style="background:{c}"></i>{label}</td><td class="v">{val}</td></tr>"#,
                c = band_color(band(m.score)),
                label = esc(&m.label),
                val = esc(&m.display),
            )
        })
        .collect();
    format!(
        r#"<section><h2>{heading}</h2><div class="rings">{rings}</div>
<table><tbody>{metrics}</tbody></table>
<p class="meta">{url} · gemessen {time} · Lighthouse {ver}</p></section>"#,
        heading = esc(heading),
        url = esc(&run.final_url),
        time = esc(&run.fetch_time),
        ver = esc(&run.lighthouse_version),
    )
}

/// Build the document. Pure — no clock, so HTML and PDF cannot disagree.
pub fn build_html(r: &PageSpeedReport) -> String {
    let mut blocks = String::new();
    if let Some(d) = &r.desktop {
        blocks.push_str(&strategy_block(d, "Desktop"));
    }
    if let Some(m) = &r.mobile {
        blocks.push_str(&strategy_block(m, "Mobil"));
    }
    if blocks.is_empty() {
        blocks.push_str(r#"<p class="warn">Keine Messung zustande gekommen.</p>"#);
    }
    // Failures are stated, never swallowed — a missing half must not read as
    // a page that simply has no data.
    let errors: String = if r.errors.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="warn">{}</p>"#,
            r.errors
                .iter()
                .map(|e| esc(e))
                .collect::<Vec<_>>()
                .join("<br>")
        )
    };

    format!(
        r#"<!doctype html>
<html lang="de"><head><meta charset="utf-8">
<title>PageSpeed — {url}</title>
<style>
  :root {{ color-scheme: light }}
  * {{ box-sizing: border-box }}
  body {{ margin:0; padding:32px 36px; background:#fff; color:#16181d;
    font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif }}
  h1 {{ margin:0 0 2px; font-size:20px; letter-spacing:-.01em }}
  .url {{ margin:0 0 26px; color:#6b7280; font:12px/1.4 ui-monospace,SFMono-Regular,Menlo,monospace;
    word-break:break-all }}
  section {{ margin-bottom:30px; padding-bottom:22px; border-bottom:1px solid #eef0f3 }}
  section:last-of-type {{ border-bottom:none }}
  h2 {{ margin:0 0 14px; font-size:15px }}
  .rings {{ display:flex; flex-wrap:wrap; gap:22px; margin-bottom:16px }}
  .ring {{ text-align:center; width:88px }}
  .ring span {{ display:block; margin-top:4px; font-size:11px; color:#6b7280 }}
  table {{ width:100%; border-collapse:collapse; font-variant-numeric:tabular-nums }}
  td {{ padding:5px 0; border-bottom:1px solid #f1f3f5 }}
  td.v {{ text-align:right; font-weight:600 }}
  td i {{ display:inline-block; width:9px; height:9px; border-radius:2px; margin-right:7px }}
  .meta {{ margin:12px 0 0; color:#9aa1ab; font-size:11px; word-break:break-all }}
  .warn {{ margin:0 0 18px; padding:9px 11px; border-radius:8px; background:#fff7ed;
    border:1px solid #fed7aa; color:#9a3412; font-size:12px }}
  footer {{ color:#9aa1ab; font-size:11px }}
</style></head><body>
<h1>PageSpeed Insights</h1>
<p class="url">{url}</p>
{errors}
{blocks}
<footer>Werte von Google PageSpeed Insights (Lighthouse) · 90–100 gut · 50–89 verbesserungswürdig · 0–49 schlecht · erstellt mit Inspector Rust</footer>
</body></html>"#,
        url = esc(&r.url),
    )
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
        assert_eq!(h.matches("<section>").count(), 2);
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
        assert_eq!(h.matches("<section>").count(), 1);
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
