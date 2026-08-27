//! `loc` report → a self-contained HTML document (v0.141.0).
//!
//! ONE renderer feeds all three export formats: the HTML is written as-is,
//! the PDF is that HTML through the existing WKWebView pipeline
//! ([`crate::md_to_pdf`]), and the PNG is the same page snapshotted. A second
//! renderer per format would drift — the repo export made the same call.
//!
//! **Self-contained by contract**: inline CSS, inline SVG, no script, no
//! external request. A report that phones home when opened would be a
//! surprise, and it has to render identically offline and inside a headless
//! webview.

use crate::loc::LocReport;

/// Escape text for HTML. Every value in the document goes through this —
/// language names and, above all, the folder path come from the filesystem.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Thousands separators, German style (the UI counts the same way).
pub fn num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Percentage of the CODE total — the same denominator the panel's share bar
/// uses, so the export can't disagree with what was on screen.
pub fn pct(part: usize, whole: usize) -> String {
    if whole == 0 || part == 0 {
        return "0,0 %".into();
    }
    format!("{:.1} %", (part as f64 / whole as f64) * 100.0).replace('.', ",")
}

/// A stable colour per language — deterministic from the name, so the same
/// project always renders in the same colours and HTML/PDF/PNG agree.
pub fn color_for(name: &str) -> String {
    let mut h: u32 = 2166136261;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    format!("hsl({}, 62%, 58%)", h % 360)
}

/// The stacked share bar: one segment per language, widths summing to 100 %.
fn share_bar(r: &LocReport) -> String {
    if r.total_code == 0 {
        return String::new();
    }
    let mut seg = String::new();
    for l in &r.languages {
        let w = (l.code as f64 / r.total_code as f64) * 100.0;
        if w < 0.05 {
            continue;
        }
        seg.push_str(&format!(
            r#"<span style="width:{w:.4}%;background:{c}" title="{n}"></span>"#,
            c = color_for(&l.name),
            n = esc(&l.name),
        ));
    }
    format!(r#"<div class="bar">{seg}</div>"#)
}

/// Build the whole document. Pure — no clock, no filesystem, so the same
/// report always produces byte-identical HTML.
pub fn build_html(r: &LocReport) -> String {
    let title = esc(&r.root_label);
    let path = r.paths.first().map(|p| esc(p)).unwrap_or_default();
    let rows: String = r
        .languages
        .iter()
        .map(|l| {
            format!(
                r#"<tr><td><i style="background:{c}"></i>{name}</td><td>{files}</td><td class="strong">{code}</td><td>{com}</td><td>{bl}</td><td>{share}</td></tr>"#,
                c = color_for(&l.name),
                name = esc(&l.name),
                files = num(l.files),
                code = num(l.code),
                com = num(l.comments),
                bl = num(l.blanks),
                share = pct(l.code, r.total_code),
            )
        })
        .collect();

    let warn = if r.inaccurate {
        r#"<p class="warn">Bei mindestens einer Datei meldete der Zähler Parse-Probleme — die Zahlen können dort abweichen.</p>"#
    } else {
        ""
    };
    let ignores = if r.respected_ignores {
        ".gitignore beachtet, versteckte Dateien übersprungen"
    } else {
        "alles gezählt (auch ignorierte und versteckte Dateien)"
    };

    format!(
        r#"<!doctype html>
<html lang="de"><head><meta charset="utf-8">
<title>Lines of code — {title}</title>
<style>
  :root {{ color-scheme: light }}
  * {{ box-sizing: border-box }}
  body {{ margin:0; padding:32px 36px; background:#fff; color:#16181d;
    font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif; }}
  h1 {{ margin:0 0 2px; font-size:20px; letter-spacing:-.01em }}
  .path {{ margin:0 0 24px; color:#6b7280; font:12px/1.4 ui-monospace,SFMono-Regular,Menlo,monospace;
    word-break:break-all }}
  .tiles {{ display:flex; flex-wrap:wrap; gap:10px; margin-bottom:22px }}
  .tile {{ flex:1 1 110px; border:1px solid #e5e7eb; border-radius:10px; padding:10px 12px }}
  .tile b {{ display:block; font-size:19px; font-variant-numeric:tabular-nums }}
  .tile span {{ font-size:11px; color:#6b7280; text-transform:uppercase; letter-spacing:.04em }}
  .bar {{ display:flex; height:10px; border-radius:5px; overflow:hidden; margin-bottom:22px;
    background:#eef0f3 }}
  .bar span {{ display:block; height:100% }}
  table {{ width:100%; border-collapse:collapse; font-variant-numeric:tabular-nums }}
  th {{ text-align:right; font-size:10px; text-transform:uppercase; letter-spacing:.05em;
    color:#6b7280; padding:0 0 6px; border-bottom:1px solid #e5e7eb }}
  th:first-child, td:first-child {{ text-align:left }}
  td {{ padding:6px 0; border-bottom:1px solid #f1f3f5; text-align:right }}
  td i {{ display:inline-block; width:9px; height:9px; border-radius:2px; margin-right:7px }}
  .strong {{ font-weight:600 }}
  tfoot td {{ font-weight:600; border-bottom:none; border-top:1px solid #e5e7eb }}
  .warn {{ margin:18px 0 0; padding:9px 11px; border-radius:8px; background:#fff7ed;
    border:1px solid #fed7aa; color:#9a3412; font-size:12px }}
  footer {{ margin-top:26px; color:#9aa1ab; font-size:11px }}
</style></head><body>
<h1>Lines of code — {title}</h1>
<p class="path">{path}</p>
<div class="tiles">
  <div class="tile"><b>{files}</b><span>Dateien</span></div>
  <div class="tile"><b>{lines}</b><span>Zeilen</span></div>
  <div class="tile"><b>{code}</b><span>Code</span></div>
  <div class="tile"><b>{comments}</b><span>Kommentare</span></div>
  <div class="tile"><b>{blanks}</b><span>Leer</span></div>
</div>
{bar}
<table>
  <thead><tr><th>Sprache</th><th>Dateien</th><th>Code</th><th>Komm.</th><th>Leer</th><th>Anteil</th></tr></thead>
  <tbody>{rows}</tbody>
  <tfoot><tr><td>Gesamt</td><td>{files}</td><td>{code}</td><td>{comments}</td><td>{blanks}</td><td>100 %</td></tr></tfoot>
</table>
{warn}
<footer>Kommentare enthalten Dokumentation (z. B. Python-Docstrings) · {ignores} · erstellt mit Inspector Rust</footer>
</body></html>"#,
        title = title,
        path = path,
        files = num(r.total_files),
        lines = num(r.total_lines),
        code = num(r.total_code),
        comments = num(r.total_comments),
        blanks = num(r.total_blanks),
        bar = share_bar(r),
        rows = rows,
        warn = warn,
        ignores = ignores,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loc::{LocLanguage, LocReport};

    fn lang(name: &str, files: usize, code: usize) -> LocLanguage {
        LocLanguage {
            name: name.into(),
            files,
            code,
            comments: 10,
            blanks: 5,
            code_pct: 0.0,
        }
    }

    fn report() -> LocReport {
        LocReport {
            root_label: "projekt".into(),
            paths: vec!["/Users/t/projekt".into()],
            respected_ignores: true,
            languages: vec![lang("Rust", 3, 800), lang("TypeScript", 2, 200)],
            total_files: 5,
            total_code: 1000,
            total_comments: 20,
            total_blanks: 10,
            total_lines: 1030,
            inaccurate: false,
            subdirs: vec![],
        }
    }

    #[test]
    fn the_document_is_self_contained() {
        let h = build_html(&report());
        // No script, and nothing that would fetch when the file is opened —
        // a report must render identically offline and in a headless webview.
        for forbidden in ["<script", "http://", "https://", "src=", "@import", "<link"] {
            assert!(!h.contains(forbidden), "{forbidden} darf nicht vorkommen");
        }
        assert!(h.starts_with("<!doctype html>"));
        assert!(h.contains("</html>"));
    }

    #[test]
    fn every_value_is_escaped_including_the_path() {
        let mut r = report();
        // Paths and language names come from the filesystem — they are the
        // injection surface here.
        r.root_label = "<script>alert(1)</script>".into();
        r.paths = vec!["/tmp/a\"b&c<d>".into()];
        r.languages[0].name = "C<++>".into();
        let h = build_html(&r);
        assert!(!h.contains("<script>alert"));
        assert!(h.contains("&lt;script&gt;"));
        assert!(h.contains("a&quot;b&amp;c&lt;d&gt;"));
        assert!(h.contains("C&lt;++&gt;"));
    }

    #[test]
    fn the_totals_and_every_language_are_present() {
        let h = build_html(&report());
        assert!(h.contains("Rust"));
        assert!(h.contains("TypeScript"));
        assert!(h.contains("1.000")); // total code, German separators
        assert!(h.contains("80,0 %")); // Rust's share of the code
    }

    #[test]
    fn it_is_deterministic_so_the_three_formats_cannot_disagree() {
        // No clock, no randomness — the same report always renders the same
        // bytes, which is what lets HTML, PDF and PNG show one truth.
        assert_eq!(build_html(&report()), build_html(&report()));
    }

    #[test]
    fn an_empty_report_renders_rather_than_dividing_by_zero() {
        let mut r = report();
        r.languages.clear();
        r.total_code = 0;
        r.total_files = 0;
        r.total_lines = 0;
        let h = build_html(&r);
        assert!(h.contains("</html>"));
        assert!(!h.contains("NaN"));
        assert!(!h.contains("inf"));
    }

    #[test]
    fn the_inaccurate_flag_is_surfaced_never_swallowed() {
        let mut r = report();
        assert!(!build_html(&r).contains("Parse-Probleme"));
        r.inaccurate = true;
        assert!(build_html(&r).contains("Parse-Probleme"));
    }

    #[test]
    fn the_footer_states_which_ignore_mode_produced_the_numbers() {
        let mut r = report();
        assert!(build_html(&r).contains(".gitignore beachtet"));
        r.respected_ignores = false;
        assert!(build_html(&r).contains("alles gezählt"));
    }

    #[test]
    fn number_and_percent_formatting() {
        assert_eq!(num(0), "0");
        assert_eq!(num(999), "999");
        assert_eq!(num(1_000), "1.000");
        assert_eq!(num(1_234_567), "1.234.567");
        assert_eq!(pct(0, 100), "0,0 %");
        assert_eq!(pct(1, 0), "0,0 %"); // never divides by zero
        assert_eq!(pct(50, 200), "25,0 %");
    }

    #[test]
    fn colours_are_stable_per_language_and_valid_css() {
        assert_eq!(color_for("Rust"), color_for("Rust"));
        assert_ne!(color_for("Rust"), color_for("TypeScript"));
        assert!(color_for("Rust").starts_with("hsl("));
    }

    #[test]
    fn a_sliver_language_does_not_emit_a_zero_width_segment() {
        let mut r = report();
        r.languages.push(lang("Winzig", 1, 0));
        let h = build_html(&r);
        assert!(!h.contains("width:0.0000%"));
        // …but it still gets a table row, so nothing is silently dropped.
        assert!(h.contains("Winzig"));
    }
}
