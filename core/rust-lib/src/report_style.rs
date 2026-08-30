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

/* ── Unterschrift + Stempel (Portal-Layout `drawSignatures`) ───── */
.rp-sign {{ display: flex; justify-content: space-between; gap: 40px; margin: 44px 0 6px; break-inside: avoid }}
.rp-sign-col {{ width: 250px; text-align: center }}
.rp-sign-space {{ position: relative; height: 68px; display: flex; align-items: flex-end; justify-content: center; font-weight: 640; font-size: 13px }}
.rp-sign-img {{ max-width: 230px; max-height: 58px; display: block; margin: 0 auto }}
.rp-seal {{ position: absolute; left: -4px; bottom: -28px; width: 106px; height: 106px }}
.rp-sign-line {{ height: 1px; background: var(--rule); margin-top: 4px }}
.rp-sign-cap {{ margin-top: 6px; font-size: 9.5px; color: var(--muted); line-height: 1.5 }}
.rp-sign-cap b {{ color: var(--ink); font-size: 11px }}

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
/// `TT.MM.JJJJ` of today — the signature's date line (the portal's dateStr).
/// The one clock read besides the seal year; the body stays deterministic.
fn today_de() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut days = (secs / 86_400) as i64;
    let mut year = 1970i64;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if days < len { break; }
        days -= len;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1;
    for len in months {
        if days < len { break; }
        days -= len;
        month += 1;
    }
    format!("{:02}.{:02}.{}", days + 1, month, year)
}

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
{sign}
<footer>{foot}</footer>
</body></html>"#,
        css = css(),
        // ⚠️ Every report gets the signature + seal through THIS one seam — the
        // per-export copy is exactly what the portal work warns against. An
        // empty foot (the bench "nothing to compare" shell) stays unsigned:
        // signing an empty document would be wrong.
        sign = if foot.is_empty() { String::new() } else { signature_block(&today_de()) },
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


// ── Unterschrift + Stempel (v0.157.0) ──────────────────────────────────────
//
// Faithful port of the portal's `drawSignatures`/`drawSeal`
// (celox-portal/server/certificate.js) into the HTML report world:
// * The SIGNATURE is the same PNG asset, vendored and embedded as a data URI —
//   the reports are pinned self-contained, an external file would break both
//   the pin and the offline guarantee.
// * The SEAL is a vector port (SVG) of `drawSeal`, geometry value for value
//   (ring radii, 52 beads, guilloche 22 lobes, shield with check, −8° tilt).
//   The portal draws it as vector precisely so no asset can go missing; SVG is
//   the same decision in this rendering world. Arc text uses SVG textPath —
//   the browser measures the glyphs, which is what pdfkit's per-char arcText
//   emulated by hand.
// * Ring wording follows the portal's honesty rule ("the stamp must not
//   contradict the sheet"): a scan is neither a completion nor an assessment,
//   so the bottom arc reads DURCHGEFÜHRTE ANALYSE.

/// Portal primary — the seal ink.
const SEAL_COLOR: &str = "#0B57D0";
const SIG_PNG: &[u8] = include_bytes!("../assets/signature.png");

/// Angles count from 12 o'clock, clockwise — the portal's convention. Using
/// math angles from the x-axis puts everything 90° off (their documented trap).
fn on_circle(r: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (a.sin() * r, -a.cos() * r)
}

/// The seal as inline SVG, radius-normalised to r = 100 (viewBox ±115).
pub fn seal_svg(bottom_text: &str) -> String {
    let c = SEAL_COLOR;
    // Beads: 52, like the portal ("Perlring … von Hand nicht sauber nachzubauen").
    let mut beads = String::new();
    for i in 0..52 {
        let (x, y) = on_circle(90.0, f64::from(i) * 360.0 / 52.0);
        beads.push_str(&format!(r##"<circle cx="{x:.2}" cy="{y:.2}" r="1.15" fill="{c}"/>"##));
    }
    // Guilloche: r 56.5 ± 5, 22 lobes, two phases, 540 steps (portal numbers —
    // "die Bogenzahl ist NICHT je mehr desto feiner").
    let guil = |phase: f64| {
        let mut d = String::from("M");
        for i in 0..=540 {
            let t = f64::from(i) / 540.0 * 360.0;
            let rr = 56.5 + 5.0 * ((22.0 * t + phase).to_radians()).cos();
            let (x, y) = on_circle(rr, t);
            if i > 0 {
                d.push_str(" L");
            }
            d.push_str(&format!("{x:.2} {y:.2}"));
        }
        d.push('Z');
        d
    };
    // Stars where the two arcs meet — upright, not radially rotated (a
    // five-point star tilted 90° vs 270° reads as a mistake, per the portal).
    let star = |cx: f64, cy: f64, r_out: f64| {
        let r_in = r_out * 0.4;
        let mut d = String::from("M");
        for i in 0..10 {
            let rr = if i % 2 == 1 { r_in } else { r_out };
            let (x, y) = on_circle(rr, f64::from(i) * 36.0);
            if i > 0 {
                d.push_str(" L");
            }
            d.push_str(&format!("{:.2} {:.2}", cx + x, cy + y));
        }
        d.push('Z');
        d
    };
    let (sx1, sy1) = on_circle(76.5, 90.0);
    let (sx2, sy2) = on_circle(76.5, 270.0);
    // Shield: portal geometry sw = 21, sh = 19, centred at (0, −10); the inner
    // tressure is the classic certificate detail.
    let shield = |w: f64, h: f64| {
        format!(
            "M{x0:.2} {y0:.2} L{x1:.2} {y0:.2} L{x1:.2} {y2:.2} Q{x1:.2} {y3:.2} 0 {y4:.2} Q{x0:.2} {y3:.2} {x0:.2} {y2:.2} Z",
            x0 = -w,
            x1 = w,
            y0 = -10.0 - h,
            y2 = -10.0 + h * 0.2,
            y3 = -10.0 + h * 0.98,
            y4 = -10.0 + h * 1.4,
        )
    };
    format!(
        r##"<svg class="rp-seal" viewBox="-115 -115 230 230" role="img" aria-label="Stempel">
<g transform="rotate(-8)" stroke="{c}" fill="none" opacity="0.92">
<circle r="100" stroke-width="4.2"/>
<circle r="94.5" stroke-width="1.3"/>
{beads}
<circle r="85.5" stroke-width="1.3"/>
<path id="rp-arc-top" d="M -76.5 0 A 76.5 76.5 0 1 1 76.5 0" stroke="none"/>
<path id="rp-arc-bot" d="M -70 0 A 70 70 0 0 0 70 0" stroke="none"/>
<text fill="{c}" stroke="none" font-family="Helvetica, Arial, sans-serif" font-weight="700" font-size="10.4" letter-spacing="2"><textPath href="#rp-arc-top" startOffset="50%" text-anchor="middle">CELOX.IO&#160;&#160;·&#160;&#160;BERLIN</textPath></text>
<text fill="{c}" stroke="none" font-family="Helvetica, Arial, sans-serif" font-weight="700" font-size="9.5" letter-spacing="1.6"><textPath href="#rp-arc-bot" startOffset="50%" text-anchor="middle">{bottom_text}</textPath></text>
<path d="{star1}" fill="{c}" stroke="none"/>
<path d="{star2}" fill="{c}" stroke="none"/>
<circle r="67.5" stroke-width="1.4"/>
<circle r="64.5" stroke-width="0.9"/>
<g opacity="0.72" stroke-width="0.8">
<path d="{g0}"/>
<path d="{g180}"/>
<circle r="56.5" stroke-width="0.6"/>
</g>
<path d="{sh_out}" stroke-width="3.8"/>
<path d="{sh_in}" stroke-width="1.05"/>
<path d="M{cx0:.2} {cy0:.2} L{cx1:.2} {cy1:.2} L{cx2:.2} {cy2:.2}" stroke-width="5.2" stroke-linecap="round" stroke-linejoin="round"/>
<text fill="{c}" stroke="none" x="0" y="34" text-anchor="middle" font-family="Helvetica, Arial, sans-serif" font-weight="700" font-size="11.8" letter-spacing="2.6">{year}</text>
</g></svg>"##,
        star1 = star(sx1, sy1, 5.0),
        star2 = star(sx2, sy2, 5.0),
        g0 = guil(0.0),
        g180 = guil(180.0),
        sh_out = shield(21.0, 19.0),
        sh_in = shield(21.0 * 0.74, 19.0 * 0.72),
        cx0 = -21.0 * 0.36,
        cy0 = -10.0 + 19.0 * 0.12,
        cx1 = -21.0 * 0.06,
        cy1 = -10.0 + 19.0 * 0.5,
        cx2 = 21.0 * 0.38,
        cy2 = -10.0 - 19.0 * 0.36,
        year = seal_year(),
    )
}

/// The seal year. The ONLY clock read in the report path — matching the portal
/// (`new Date().getFullYear()`); everything else stays deterministic.
fn seal_year() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = secs / 86_400;
    let mut year = 1970i32;
    let mut rem = days as i64;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if rem < len {
            break;
        }
        rem -= len;
        year += 1;
    }
    year
}

/// Signature + seal footer block — the portal's `drawSignatures` layout: date
/// left over a line, the signature over the right line with the seal ON the
/// ink ("ein Stempel liegt auf der Tinte, nicht darunter"), name + issuer
/// below. ⚠️ Deliberately NO "digital signiert": these PDFs carry no
/// cryptographic signature, and the portal documents why that claim would be
/// exactly what a careful reader checks.
pub fn signature_block(date: &str) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let sig = B64.encode(SIG_PNG);
    format!(
        r##"<div class="rp-sign">
  <div class="rp-sign-col">
    <div class="rp-sign-space">{date}</div>
    <div class="rp-sign-line"></div>
    <div class="rp-sign-cap">Datum</div>
  </div>
  <div class="rp-sign-col">
    <div class="rp-sign-space"><img class="rp-sign-img" src="data:image/png;base64,{sig}" alt="Unterschrift Martin Pfeffer">{seal}</div>
    <div class="rp-sign-line"></div>
    <div class="rp-sign-cap"><b>Martin Pfeffer</b><br>Erstellt mit Inspector Rust · celox.io</div>
  </div>
</div>"##,
        seal = seal_svg("DURCHGEFÜHRTE ANALYSE"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip embedded data URIs before content assertions: the base64 blob of
    /// the signature can contain ANY letter triple — "NaN" included — and a
    /// grep over it produces false positives. External-reference checks must
    /// still see everything else, so only the base64 payload is removed.
    fn sans_data_uris(h: &str) -> String {
        let mut out = String::new();
        let mut rest = h;
        while let Some(i) = rest.find("data:image/png;base64,") {
            out.push_str(&rest[..i]);
            out.push_str("data:image/png;base64,ELIDED");
            let tail = &rest[i + 22..];
            let end = tail.find('"').unwrap_or(tail.len());
            rest = &tail[end..];
        }
        out.push_str(rest);
        out
    }

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
        // The signed shell must carry signature + seal — as MARKUP, not just
        // the class names, which always sit in the stylesheet (first version of
        // this pin grepped bare "rp-sign" and matched the CSS).
        assert!(h.contains(r#"<div class="rp-sign">"#) && h.contains(r#"<svg class="rp-seal""#));
        // … and an UNSIGNED shell (empty foot = the bench "nothing to compare"
        // case) must not sign an empty document.
        assert!(!shell("K", "T", "", "<p/>", "").contains(r#"<div class="rp-sign">"#));
        // Self-contained: nothing that would fetch when the file is opened.
        // ⚠️ The embedded signature legitimately uses src="data:…".
        let h = sans_data_uris(&h).replace("src=\"data:image/png;base64,ELIDED", "");
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


    #[test]
    fn the_block_carries_signature_seal_and_name() {
        let h = signature_block("30.08.2026");
        assert!(h.contains("data:image/png;base64,"), "embedded signature");
        assert!(h.contains("rp-seal"), "seal svg present");
        assert!(h.contains("Martin Pfeffer"));
        assert!(h.contains("30.08.2026"));
        // ⚠️ The portal explicitly refuses this claim: no cryptographic
        // signature, so the words must not appear.
        assert!(!h.to_lowercase().contains("digital signiert"));
    }

    #[test]
    fn the_seal_keeps_the_portal_geometry_and_honest_wording() {
        let s = seal_svg("DURCHGEFÜHRTE ANALYSE");
        assert!(s.contains("rotate(-8)"), "hand-stamped tilt");
        assert_eq!(s.matches("<circle cx=").count(), 52, "52 beads");
        assert!(s.contains("CELOX.IO"));
        assert!(s.contains("DURCHGEFÜHRTE ANALYSE"));
        // ⚠️ The stamp must not contradict the sheet: a scan is neither a
        // completion nor an assessment (the portal's documented rule).
        assert!(!s.contains("ABSCHLUSS") && !s.contains("ASSESSMENT"));
        assert!(s.contains(SEAL_COLOR));
    }

    #[test]
    fn the_seal_is_pure_vector_apart_from_text() {
        // The portal draws the seal as vector so no asset can go missing; the
        // SVG port must not smuggle a bitmap in.
        let s = seal_svg("X");
        assert!(!s.contains("<img") && !s.contains("data:image"));
    }
}
