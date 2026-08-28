//! The shared look of every exported report (v0.143.0).
//!
//! Five documents used to carry four hand-written stylesheets — two of them
//! DARK (`repo`, timesheet) and two light — which is the opposite of a house
//! style, and a dark report is the wrong artefact anyway: these are pages you
//! print, attach to a mail, or hand a client.
//!
//! # The rules this encodes
//!
//! * **Print-first, light.** One page geometry (A4, 14 mm), one ink colour.
//! * **Colour only where it carries data.** There is no decorative accent:
//!   the Lighthouse bands and the language hues mean something, the rest is
//!   ink, muted ink and a hairline. That is what keeps a dense page calm.
//! * **Numbers are a column, not prose.** `tabular-nums` everywhere,
//!   right-aligned, so figures line up digit under digit and can be compared
//!   by eye — the single biggest legibility win in a data report.
//! * **Data-ink over chrome.** Hairlines and whitespace instead of boxes;
//!   no vertical rules, no zebra stripes.
//! * ⚠️ **`print-color-adjust: exact` is load-bearing.** Without it WebKit
//!   drops background colours when producing the PDF, and every share bar,
//!   band chip and ring flat-out vanishes — the report silently loses exactly
//!   the parts that carry meaning.
//! * ⚠️ **Self-contained.** Inline CSS, inline SVG, no script, no external
//!   request: a report must render identically offline, inside a headless
//!   webview, and in three years.

/// A categorical palette for series colours (languages, projects …).
///
/// ⚠️ **Not a raw hue from a hash.** Hashing a name straight onto 0–360°
/// gives stability but no separation — two languages in the same report came
/// out as near-identical teals. These twelve are picked to stay
/// distinguishable next to each other and legible on white; a name still maps
/// deterministically into the set, so a project keeps its colour between
/// reports.
pub const SERIES: [&str; 12] = [
    "#3f6cd4", "#e0653f", "#2f9e6f", "#b357c9", "#d9a520", "#3aa3bf",
    "#c94f6d", "#6b7ae0", "#57913a", "#c1682a", "#8a63d2", "#2f8f8f",
];

/// Deterministic colour for a series name.
pub fn series_color(name: &str) -> &'static str {
    let mut h: u32 = 2166136261;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    SERIES[(h as usize) % SERIES.len()]
}

/// Ink, muted ink, hairline — the whole non-semantic palette.
pub const INK: &str = "#14161a";
pub const MUTED: &str = "#6a7078";
pub const RULE: &str = "#e7e9ee";

/// The stylesheet. One string, shared by every report.
pub fn css() -> String {
    format!(
        r#":root {{ color-scheme: light; --ink:{INK}; --muted:{MUTED}; --rule:{RULE}; }}
*, *::before, *::after {{ box-sizing: border-box }}
html {{ -webkit-text-size-adjust: 100% }}
body {{
  margin: 0 auto; max-width: 860px; padding: 40px 44px 56px;
  background: #fff; color: var(--ink);
  font: 13.5px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, Helvetica, Arial, sans-serif;
  font-variant-numeric: tabular-nums; font-feature-settings: "tnum" 1, "cv05" 1;
  -webkit-font-smoothing: antialiased;
}}

/* ── Document head ─────────────────────────────────────────────── */
.rp-head {{ margin-bottom: 30px }}
.rp-kicker {{
  margin: 0 0 6px; font-size: 10.5px; font-weight: 600; letter-spacing: .09em;
  text-transform: uppercase; color: var(--muted);
}}
h1 {{ margin: 0; font-size: 23px; line-height: 1.2; font-weight: 640; letter-spacing: -.016em }}
.rp-sub {{
  margin: 7px 0 0; color: var(--muted); font-size: 12px; line-height: 1.45;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace; word-break: break-all;
}}
.rp-rule {{ height: 1px; background: var(--rule); margin: 22px 0 26px }}

/* ── Section ───────────────────────────────────────────────────── */
section {{ margin: 0 0 34px; break-inside: avoid }}
h2 {{
  margin: 0 0 14px; font-size: 12px; font-weight: 640; letter-spacing: .05em;
  text-transform: uppercase; color: var(--muted);
  display: flex; align-items: baseline; gap: 8px;
}}
h2::after {{ content: ""; flex: 1; height: 1px; background: var(--rule) }}

/* ── Stat strip: label above, figure below, hairline between ───── */
.rp-stats {{ display: flex; flex-wrap: wrap; gap: 0; margin: 0 0 26px }}
.rp-stat {{ flex: 1 1 96px; padding: 0 16px; border-left: 1px solid var(--rule) }}
.rp-stat:first-child {{ padding-left: 0; border-left: none }}
.rp-stat .l {{
  display: block; font-size: 10px; font-weight: 600; letter-spacing: .07em;
  text-transform: uppercase; color: var(--muted); margin-bottom: 3px;
}}
.rp-stat .v {{ display: block; font-size: 21px; font-weight: 620; letter-spacing: -.02em; line-height: 1.15 }}
.rp-stat .u {{ font-size: 12px; font-weight: 500; color: var(--muted); margin-left: 2px }}

/* ── Stacked share bar ─────────────────────────────────────────── */
.rp-bar {{ display: flex; height: 9px; border-radius: 5px; overflow: hidden; background: #f1f3f6; margin: 0 0 10px }}
.rp-bar > span {{ display: block; height: 100% }}
.rp-legend {{ display: flex; flex-wrap: wrap; gap: 4px 16px; margin: 0 0 26px; font-size: 11.5px; color: var(--muted) }}
.rp-legend span {{ display: inline-flex; align-items: center; gap: 6px }}
.rp-legend i {{ width: 8px; height: 8px; border-radius: 2px; flex: none }}
.rp-legend b {{ color: var(--ink); font-weight: 600 }}

/* ── Table: hairline rows, no vertical rules, numbers right ────── */
table {{ width: 100%; border-collapse: collapse }}
thead {{ display: table-header-group }}   /* repeat the head across PDF pages */
th {{
  font-size: 10px; font-weight: 600; letter-spacing: .07em; text-transform: uppercase;
  color: var(--muted); text-align: right; padding: 0 0 7px; border-bottom: 1px solid var(--rule);
  white-space: nowrap;
}}
th:first-child, td:first-child {{ text-align: left }}
td {{ padding: 7px 0; border-bottom: 1px solid #f2f4f7; text-align: right; vertical-align: baseline }}
td:first-child {{ padding-right: 14px }}
/* ⚠️ Eine Rinne zwischen JEDEM Spaltenpaar. Ohne sie stößt eine rechtsbündige
   Zahlenspalte direkt an die nächste linksbündige Textspalte — gemessen:
   "2.5010:00:00–12:30:00". Bei loc/repo fiel das nie auf, weil dort die
   Zahlen die letzten Spalten sind. */
th + th, td + td {{ padding-left: 18px }}
tr:last-child td {{ border-bottom: none }}
tfoot td {{ font-weight: 640; border-top: 1px solid var(--rule); border-bottom: none; padding-top: 9px }}
.rp-num {{ font-weight: 600 }}
/* ⚠️ Nur `:first-child` ist von Haus aus linksbündig — das genügt, solange ein
   Report genau EINE Textspalte hat (loc, repo). Ein Report mit mehreren
   (die Zeiterfassung: Projekt, App, Host, Tätigkeit) schob sie sonst alle nach
   rechts, wo sie wie Zahlen aussahen. */
.rp-text {{ text-align: left }}
/* Neutraler Erklärsatz unter einer Abschnittsüberschrift. NICHT `.rp-note` —
   das ist der bernsteinfarbene Warnkasten und liest sich als Vorbehalt. */
.rp-lede {{ color: var(--muted); font-size: 12.5px; margin: 0 0 14px; max-width: 62ch }}
.rp-dim {{ color: var(--muted) }}
/* The share bar lives INSIDE the label cell — the proportion sits next to
   the name instead of in a separate column that has to be scanned. */
.rp-name {{ position: relative; display: block; padding: 2px 0 }}
.rp-name i {{ display: inline-block; width: 8px; height: 8px; border-radius: 2px; margin-right: 9px; vertical-align: baseline }}
.rp-track {{ position: absolute; left: 17px; right: 0; bottom: -3px; height: 2px; background: #f2f4f7; border-radius: 1px }}
.rp-track > span {{ display: block; height: 100%; border-radius: 1px }}

/* ── Notices ───────────────────────────────────────────────────── */
.rp-note {{
  margin: 0 0 22px; padding: 10px 13px; border-radius: 9px; font-size: 12px; line-height: 1.5;
  background: #fff8ed; border: 1px solid #fbdba7; color: #8a4b08;
}}
.rp-empty {{ color: var(--muted); font-size: 12.5px; margin: 0 0 22px }}

/* ── Foot ──────────────────────────────────────────────────────── */
footer {{
  margin-top: 40px; padding-top: 14px; border-top: 1px solid var(--rule);
  color: var(--muted); font-size: 10.5px; line-height: 1.6;
}}
footer b {{ color: var(--ink); font-weight: 600 }}

/* ── Print ─────────────────────────────────────────────────────── */
@page {{ size: A4; margin: 14mm }}
@media print {{
  /* ⚠️ Without this WebKit drops every background when rendering the PDF —
     share bars, band chips and rings simply disappear. */
  * {{ -webkit-print-color-adjust: exact; print-color-adjust: exact }}
  body {{ max-width: none; padding: 0 }}
  section, .rp-stats, table {{ break-inside: avoid }}
  tr {{ break-inside: avoid }}
  footer {{ break-before: avoid }}
}}"#
    )
}

/// Wrap content in the shared document shell.
///
/// `kicker` is the small label above the title (what KIND of report this is),
/// `subject` the thing measured (path, URL, range) — shown monospaced because
/// it is a machine string a reader may need to copy exactly.
pub fn shell(kicker: &str, title: &str, subject: &str, body: &str, foot: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="de"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — {subject_plain}</title>
<style>
{css}
</style></head><body>
<header class="rp-head">
  <p class="rp-kicker">{kicker}</p>
  <h1>{title}</h1>
  <p class="rp-sub">{subject}</p>
</header>
<div class="rp-rule"></div>
{body}
<footer>{foot}</footer>
</body></html>"#,
        css = css(),
        kicker = kicker,
        title = title,
        subject = subject,
        // The <title> must not carry markup; the escaped subject is already
        // safe, it just reads badly with entities in a window title.
        subject_plain = subject,
    )
}

/// A share (0..1) as a German percentage: `0.118` → `"11,8 %"`.
///
/// ⚠️ Formats the NUMBER on its own. A blanket `.replace('.', ",")` over a
/// finished line also hits the label — a language called "Node.js" came out as
/// "Node,js". The rule lived in three places before this helper existed, which
/// is exactly how one of them ends up with a different decimal separator than
/// the legend right next to it (measured: "11,8 %" beside "11.8 %").
pub fn pct(share: f64) -> String {
    format!("{:.1} %", share * 100.0).replace('.', ",")
}

/// One entry of the stat strip. `unit` is rendered smaller and muted, so
/// "3.963 Zeilen" reads as one figure rather than two.
pub struct Stat<'a> {
    pub label: &'a str,
    pub value: String,
    pub unit: Option<&'a str>,
}

pub fn stats(items: &[Stat<'_>]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let cells: String = items
        .iter()
        .map(|s| {
            format!(
                r#"<div class="rp-stat"><span class="l">{l}</span><span class="v">{v}{u}</span></div>"#,
                l = s.label,
                v = s.value,
                u = s
                    .unit
                    .map(|u| format!(r#"<span class="u">{u}</span>"#))
                    .unwrap_or_default(),
            )
        })
        .collect();
    format!(r#"<div class="rp-stats">{cells}</div>"#)
}

/// A stacked share bar plus its legend. `parts` is `(label, share 0..1,
/// colour)`; shares below half a percent are folded out of the BAR (they
/// would be invisible slivers) but never out of the legend.
pub fn share_bar(parts: &[(String, f64, String)]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    let segs: String = parts
        .iter()
        .filter(|(_, share, _)| *share >= 0.005)
        .map(|(_, share, color)| {
            format!(r#"<span style="width:{:.3}%;background:{color}"></span>"#, share * 100.0)
        })
        .collect();
    let legend: String = parts
        .iter()
        .map(|(label, share, color)| {
            let p = pct(*share);
            format!(r#"<span><i style="background:{color}"></i>{label} <b>{p}</b></span>"#)
        })
        .collect();
    format!(r#"<div class="rp-bar">{segs}</div><div class="rp-legend">{legend}</div>"#)
}

/// A table row's label cell: colour chip, name, and a hairline share track
/// underneath — the proportion sits WITH the name instead of in a column the
/// eye has to travel to.
pub fn name_cell(color: &str, name: &str, share: f64) -> String {
    format!(
        r#"<span class="rp-name"><i style="background:{color}"></i>{name}<span class="rp-track"><span style="width:{:.2}%;background:{color}"></span></span></span>"#,
        (share * 100.0).clamp(0.0, 100.0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_colours_are_stable_distinct_and_legible() {
        // Stability: the same name always gets the same colour, so a project
        // keeps its identity across reports.
        assert_eq!(series_color("Rust"), series_color("Rust"));
        // Distinctness within the palette itself — the failure that started
        // this: two languages rendered as near-identical teals.
        let uniq: std::collections::HashSet<_> = SERIES.iter().collect();
        assert_eq!(uniq.len(), SERIES.len(), "Palette enthält Dubletten");
        for c in SERIES {
            assert!(c.starts_with('#') && c.len() == 7, "{c}");
            // Dark enough to read as a chip on white: no near-white entries.
            let lum: u32 = (1..7)
                .step_by(2)
                .map(|i| u32::from_str_radix(&c[i..i + 2], 16).unwrap())
                .sum();
            assert!(lum < 3 * 210, "{c} ist zu hell für weißen Grund");
        }
    }

    #[test]
    fn the_shell_is_a_complete_self_contained_document() {
        let h = shell("Kicker", "Titel", "/pfad", "<p>x</p>", "Fuß");
        assert!(h.starts_with("<!doctype html>"));
        assert!(h.contains("</html>"));
        // Self-contained: nothing that would fetch when the file is opened.
        for forbidden in ["<script", "http://", "https://", "@import", "<link", "src="] {
            assert!(!h.contains(forbidden), "{forbidden} darf nicht vorkommen");
        }
        assert!(h.contains("Kicker") && h.contains("Titel") && h.contains("/pfad") && h.contains("Fuß"));
    }

    #[test]
    fn print_colour_adjust_is_present_in_every_report() {
        // ⚠️ Without it the PDF silently loses every share bar, band chip and
        // ring — exactly the parts that carry the meaning.
        let c = css();
        assert!(c.contains("print-color-adjust: exact"));
        assert!(c.contains("-webkit-print-color-adjust: exact"));
    }

    #[test]
    fn the_page_is_a4_and_tables_repeat_their_head() {
        let c = css();
        assert!(c.contains("size: A4"), "A4, nicht Letter — die Leser sind hier");
        // A table running over a page break must not lose its header.
        assert!(c.contains("display: table-header-group"));
        assert!(c.contains("break-inside: avoid"));
    }

    #[test]
    fn numbers_are_tabular_so_columns_line_up() {
        assert!(css().contains("tabular-nums"));
    }

    #[test]
    fn the_stat_strip_renders_labels_values_and_optional_units() {
        let s = stats(&[
            Stat { label: "Dateien", value: "32".into(), unit: None },
            Stat { label: "Dauer", value: "7,5".into(), unit: Some("h") },
        ]);
        assert!(s.contains("Dateien") && s.contains(">32<"));
        assert!(s.contains(r#"<span class="u">h</span>"#));
        // Empty input must not emit an empty container that draws a stray rule.
        assert_eq!(stats(&[]), "");
    }

    #[test]
    fn a_sliver_keeps_its_legend_entry_but_not_a_zero_width_segment() {
        let out = share_bar(&[
            ("Rust".into(), 0.8, "#f00".into()),
            ("Winzig".into(), 0.001, "#0f0".into()),
        ]);
        // The bar drops the invisible sliver…
        assert_eq!(out.matches("rp-bar").count(), 1);
        assert!(!out.contains("width:0,100%") && !out.contains("width:0.100%"));
        // …but the legend still names it, so nothing vanishes silently.
        assert!(out.contains("Winzig"));
        // German decimal comma in the percentages.
        assert!(out.contains("80,0 %"));
        assert_eq!(share_bar(&[]), "");
    }

    #[test]
    fn the_name_cell_carries_its_share_as_a_track() {
        let c = name_cell("#abc", "Rust", 0.42);
        assert!(c.contains("Rust") && c.contains("#abc"));
        assert!(c.contains("width:42.00%"));
        // Out-of-range shares are clamped rather than overflowing the row.
        assert!(name_cell("#abc", "x", 2.5).contains("width:100.00%"));
        assert!(name_cell("#abc", "x", -1.0).contains("width:0.00%"));
    }
}
