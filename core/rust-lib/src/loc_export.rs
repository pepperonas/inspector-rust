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
use crate::report_style as rs;

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
    rs::pct(part as f64 / whole as f64)
}

/// A stable colour per language, from the shared categorical palette — so a
/// language keeps its colour between reports AND stays distinguishable from
/// its neighbours (a raw hashed hue gave neither).
pub fn color_for(name: &str) -> String {
    rs::series_color(name).to_string()
}

/// Build the whole document. Pure — no clock, no filesystem, so the same
/// report always produces byte-identical HTML.
pub fn build_html(r: &LocReport) -> String {
    let parts: Vec<(String, f64, String)> = r
        .languages
        .iter()
        .map(|l| {
            (
                esc(&l.name),
                if r.total_code > 0 { l.code as f64 / r.total_code as f64 } else { 0.0 },
                color_for(&l.name),
            )
        })
        .collect();

    let rows: String = r
        .languages
        .iter()
        .map(|l| {
            let share = if r.total_code > 0 { l.code as f64 / r.total_code as f64 } else { 0.0 };
            format!(
                r#"<tr><td>{name}</td><td class="rp-dim">{files}</td><td class="rp-num">{code}</td><td class="rp-dim">{com}</td><td class="rp-dim">{bl}</td><td>{share}</td></tr>"#,
                name = rs::name_cell(&color_for(&l.name), &esc(&l.name), share),
                files = num(l.files),
                code = num(l.code),
                com = num(l.comments),
                bl = num(l.blanks),
                share = pct(l.code, r.total_code),
            )
        })
        .collect();

    let table = if r.languages.is_empty() {
        r#"<p class="rp-empty">In diesem Ordner wurde kein zählbarer Code gefunden.</p>"#.to_string()
    } else {
        format!(
            r#"<section><h2>Nach Sprache</h2>
{bar}
<table>
<thead><tr><th>Sprache</th><th>Dateien</th><th>Code</th><th>Kommentare</th><th>Leer</th><th>Anteil</th></tr></thead>
<tbody>{rows}</tbody>
<tfoot><tr><td>Gesamt</td><td>{files}</td><td>{code}</td><td>{com}</td><td>{bl}</td><td>100 %</td></tr></tfoot>
</table></section>"#,
            bar = rs::share_bar(&parts),
            files = num(r.total_files),
            code = num(r.total_code),
            com = num(r.total_comments),
            bl = num(r.total_blanks),
        )
    };

    let warn = if r.inaccurate {
        r#"<p class="rp-note">Bei mindestens einer Datei meldete der Zähler <b>Parse-Probleme</b> — dort können die Zahlen abweichen.</p>"#
    } else {
        ""
    };

    let body = format!(
        "{warn}{stats}{table}",
        stats = rs::stats(&[
            rs::Stat { label: "Dateien", value: num(r.total_files), unit: None },
            rs::Stat { label: "Zeilen", value: num(r.total_lines), unit: None },
            rs::Stat { label: "Code", value: num(r.total_code), unit: None },
            rs::Stat { label: "Kommentare", value: num(r.total_comments), unit: None },
            rs::Stat { label: "Leer", value: num(r.total_blanks), unit: None },
        ]),
    );

    let ignores = if r.respected_ignores {
        "<b>.gitignore</b> beachtet, versteckte Dateien übersprungen"
    } else {
        "<b>alles</b> gezählt — auch ignorierte und versteckte Dateien"
    };
    let foot = format!(
        "Gezählt mit tokei. <b>Kommentare</b> enthalten Dokumentation (z. B. Python-Docstrings); \
         <b>Zeilen</b> = Code + Kommentare + Leerzeilen. {ignores}.<br>Erstellt mit Inspector Rust."
    );

    rs::shell(
        "Lines of Code",
        &esc(&r.root_label),
        &r.paths.first().map(|p| esc(p)).unwrap_or_default(),
        &body,
        &foot,
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

    /// Offline sight check — see the timesheet dump for why.
    #[test]
    #[ignore]
    fn dump_for_a_sight_check() {
        let dir = std::path::PathBuf::from(
            std::env::var("IR_DUMP_DIR").unwrap_or_else(|_| "/tmp".into()),
        );
        std::fs::write(dir.join("loc-report.html"), build_html(&report())).unwrap();
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
    fn a_report_with_a_zero_total_never_prints_infinity() {
        // ⚠️ Der Vorgänger hieß "…rather_than_dividing_by_zero", räumte aber
        // mit `languages.clear()` genau die Daten weg, die die Division
        // auslösen. Er war GRÜN, auch wenn man den Nenner-Schutz in `pct`
        // ersatzlos entfernte — nachgemessen. Hier bleiben die Sprachen stehen
        // und nur der Nenner ist 0, das ist der Fall, den es zu fangen gilt.
        let mut r = report();
        r.total_code = 0;
        r.total_files = 0;
        r.total_lines = 0;
        let h = build_html(&r);
        let body = h.split("</style>").nth(1).expect("Dokument hat ein Stylesheet");
        assert!(!body.contains("NaN"), "NaN im Inhalt");
        assert!(!body.contains("inf"), "Unendlich im Inhalt");
        assert!(body.contains("Rust"), "die Sprachen müssen im Inhalt stehen");
    }

    #[test]
    fn an_empty_report_renders_rather_than_failing() {
        let mut r = report();
        r.languages.clear();
        r.total_code = 0;
        r.total_files = 0;
        r.total_lines = 0;
        let h = build_html(&r);
        assert!(h.contains("</html>"));
        // ⚠️ Nur den INHALT prüfen, nicht das Stylesheet. Die Zusicherung gilt
        // den ausgegebenen ZAHLEN — als blinde Teilzeichenketten-Suche über das
        // ganze Dokument scheiterte sie am Wort "bernsteinfarbene" in einem
        // CSS-Kommentar. Ein Test, der an deutscher Prosa zerbricht, prüft
        // nicht, was er zu prüfen vorgibt.
        let body = h.split("</style>").nth(1).expect("Dokument hat ein Stylesheet");
        assert!(!body.contains("NaN"), "NaN im Inhalt");
        assert!(!body.contains("inf"), "Unendlich im Inhalt");
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
        // The wording carries markup now (the shared shell renders it), so
        // assert on the distinguishing words, not on a formatted sentence.
        let on = build_html(&r);
        assert!(on.contains("beachtet") && on.contains("versteckte Dateien übersprungen"));
        assert!(!on.contains("auch ignorierte"));
        r.respected_ignores = false;
        let off = build_html(&r);
        assert!(off.contains("auch ignorierte und versteckte Dateien"));
        assert!(!off.contains("übersprungen"));
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
        // Palette colours now, not raw hues — a hex from the shared set.
        assert!(color_for("Rust").starts_with('#') && color_for("Rust").len() == 7);
        assert!(rs::SERIES.contains(&color_for("Rust").as_str()));
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
