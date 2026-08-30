//! Bruno → HTML/PDF export, in the shared `report_style` design (v0.156.0).
//!
//! Bruno computes in the FRONTEND (`lib/bruno.ts`), so the finished breakdown
//! crosses IPC as a `BrunoReport` and this module only renders — one renderer
//! for both formats, like every other export (`loc`, `pagespeed`, `bench`).
//! No tax value is computed or defaulted here: a field the frontend did not
//! send is a field the report does not show.

use crate::report_style as rs;

/// Colour of the "Netto" segment in the composition bar.
///
/// ⚠️ Deliberately light and NOT from `report_style::SERIES`: the deductions
/// take their colours from that palette, and the first draft gave Netto the
/// accent blue — sight check showed Netto and Lohnsteuer as two near-identical
/// blues side by side, the two LARGEST segments of the bar. Light = what stays
/// yours, saturated = what is taken; that contrast is the bar's whole point.
const NET_COLOR: &str = "#b9c7e6";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `48123.4` → `48.123 €` (German grouping, whole euros — the calculator's own
/// row rendering rounds to whole euros too, and a tax estimate with cents would
/// suggest a precision the simplified §32a tariff does not have).
fn eur(v: f64) -> String {
    let n = v.round() as i64;
    let neg = n < 0;
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    format!("{}{} €", if neg { "−" } else { "" }, out)
}

/// One deduction row. `label`/`value` come from the frontend verbatim.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrunoLine {
    pub label: String,
    pub value: f64,
}

/// The finished breakdown, exactly as the popup showed it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrunoReport {
    /// "employee" | "self" — decides wording only, never math.
    pub mode: String,
    /// Yearly gross (employee) or yearly profit (self-employed).
    pub base_year: f64,
    pub net_year: f64,
    pub net_month: f64,
    /// 0..1 share of the base that goes to deductions.
    pub deduction_rate: f64,
    /// 0..1 marginal rate.
    pub marginal_rate: f64,
    /// Tax rows (Lohn-/Einkommensteuer, Soli, Kirchensteuer, GewSt …).
    pub taxes: Vec<BrunoLine>,
    /// Social-insurance rows (KV, PV, RV, AV — whatever the mode has).
    pub social: Vec<BrunoLine>,
    /// The assumptions line the popup shows (Steuerklasse/Bundesland/… or the
    /// self-employed variant). Shown verbatim, escaped.
    pub assumptions: String,
}

/// Deterministic, self-contained document — inline CSS via `report_style`,
/// no script, no external request (test-pinned like the sibling exports).
pub fn build_html(r: &BrunoReport) -> String {
    let is_self = r.mode == "self";
    let title = if is_self { "Netto-Rechnung — Unternehmer" } else { "Netto-Rechnung — Angestellt" };
    let base_label = if is_self { "Gewinn / Jahr" } else { "Brutto / Jahr" };

    let stats = rs::stats(&[
        rs::Stat { label: "Netto / Monat", value: eur(r.net_month), unit: None },
        rs::Stat { label: "Netto / Jahr", value: eur(r.net_year), unit: None },
        rs::Stat { label: base_label, value: eur(r.base_year), unit: None },
        rs::Stat { label: "Abgabenquote", value: rs::pct(r.deduction_rate), unit: None },
        rs::Stat { label: "Grenzsteuersatz", value: rs::pct(r.marginal_rate), unit: None },
    ]);

    // ── Composition bar: where the gross goes. Net first, then every row. ──
    // Colors come from the shared curated palette, keyed by the row label so a
    // deduction keeps its color between employee and self reports.
    let mut parts: Vec<(String, f64, String)> = Vec::new();
    if r.base_year > 0.0 {
        parts.push(("Netto".into(), r.net_year / r.base_year, NET_COLOR.into()));
        for line in r.taxes.iter().chain(&r.social) {
            if line.value > 0.0 {
                parts.push((
                    line.label.clone(),
                    line.value / r.base_year,
                    rs::series_color(&line.label).to_string(),
                ));
            }
        }
    }
    let bar = rs::share_bar(&parts);

    // ── Table: every row, amount + share of the base. Zero rows stay listed
    // (a Soli of 0 € is a STATEMENT under the 2025 tariff, not noise) — but
    // only for rows the mode actually has; absent rows were never sent. ──
    let row = |line: &BrunoLine| {
        let share = if r.base_year > 0.0 { line.value / r.base_year } else { 0.0 };
        // ⚠️ `name_cell` returns a <span> that belongs INSIDE a <td> (see
        // loc_export). Without the wrapping cell the browser hoists the span
        // clean out of the table — sight check showed every label rendered
        // ABOVE the table while the amounts sat in nameless rows.
        format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
            rs::name_cell(rs::series_color(&line.label), &esc(&line.label), share),
            eur(line.value),
            rs::pct(share),
        )
    };
    let taxes_rows: String = r.taxes.iter().map(row).collect();
    let social_rows: String = r.social.iter().map(row).collect();
    let total_ded: f64 = r.taxes.iter().chain(&r.social).map(|l| l.value).sum();

    let body = format!(
        r#"{stats}
<section>
  <h2>Zusammensetzung</h2>
  {bar}
</section>
<section>
  <h2>Steuern</h2>
  <table><thead><tr><th>Posten</th><th>Betrag</th><th>Anteil</th></tr></thead>
  <tbody>{taxes_rows}</tbody></table>
</section>
<section>
  <h2>Sozialabgaben</h2>
  <table><thead><tr><th>Posten</th><th>Betrag</th><th>Anteil</th></tr></thead>
  <tbody>{social_rows}</tbody></table>
</section>
<section>
  <h2>Ergebnis</h2>
  <table><tbody>
    <tr><td class="rp-text">{base_label}</td><td>{base}</td><td></td></tr>
    <tr><td class="rp-text">Abzüge gesamt</td><td>−{ded}</td><td>{dedp}</td></tr>
    <tr><td class="rp-text"><b>Netto / Jahr</b></td><td><b>{net}</b></td><td>{netp}</td></tr>
    <tr><td class="rp-text">Netto / Monat</td><td>{netm}</td><td></td></tr>
  </tbody></table>
</section>"#,
        base = eur(r.base_year),
        ded = eur(total_ded),
        dedp = rs::pct(if r.base_year > 0.0 { total_ded / r.base_year } else { 0.0 }),
        net = eur(r.net_year),
        netp = rs::pct(if r.base_year > 0.0 { r.net_year / r.base_year } else { 0.0 }),
        netm = eur(r.net_month),
    );

    rs::shell(
        "Inspector Rust · Brutto → Netto",
        title,
        &format!("{} · Steuerjahr 2025", esc(&r.assumptions)),
        &body,
        "Vereinfachter §32a-Tarif (Steuerjahr 2025) · Schätzung, keine Steuerberatung · Erstellt mit Inspector Rust.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(is_self: bool) -> BrunoReport {
        BrunoReport {
            mode: if is_self { "self" } else { "employee" }.into(),
            base_year: 60000.0,
            net_year: 37795.0,
            net_month: 3149.6,
            deduction_rate: 0.37,
            marginal_rate: 0.4207,
            taxes: vec![
                BrunoLine { label: "Lohnsteuer".into(), value: 9290.0 },
                BrunoLine { label: "Soli".into(), value: 0.0 },
            ],
            // ⚠️ These are the REAL verified numbers from computeBruno for
            // 60.000 € (Klasse I, NRW, 2025) — a fixture whose rows do not sum
            // to base − net renders a document whose own arithmetic is visibly
            // wrong (sight check: the bar had a gap, 63,0 % + 26,4 % ≠ 100 %).
            social: vec![
                BrunoLine { label: "Krankenversicherung".into(), value: 5115.0 },
                BrunoLine { label: "Pflegeversicherung".into(), value: 1440.0 },
                BrunoLine { label: "Rentenversicherung".into(), value: 5580.0 },
                BrunoLine { label: "Arbeitslosenversicherung".into(), value: 780.0 },
            ],
            assumptions: "Steuerklasse I · NRW · kinderlos <& Co>".into(),
        }
    }

    #[test]
    fn the_net_segment_never_shares_a_palette_colour() {
        // The deductions draw from the shared SERIES palette; if Netto did too
        // the bar's two largest segments could render near-identically (that is
        // exactly what the first sight check showed).
        assert!(!rs::SERIES.contains(&NET_COLOR));
        assert!(build_html(&sample(false)).contains(NET_COLOR));
    }

    #[test]
    fn the_fixture_arithmetic_holds_together() {
        // ⚠️ base − Σ(rows) must equal net, or the rendered document contradicts
        // itself (the bar shows a gap and the totals row disagrees with the
        // stat strip). The real pipeline is pinned in TS (`buildBrunoExport`);
        // this keeps the SIGHT fixture honest too.
        let s = sample(false);
        let ded: f64 = s.taxes.iter().chain(&s.social).map(|l| l.value).sum();
        assert!((s.base_year - ded - s.net_year).abs() < 1.0, "{}", s.base_year - ded);
    }

    #[test]
    fn the_report_is_self_contained_and_deterministic() {
        let h = build_html(&sample(false));
        // Same contract as loc/pagespeed/bench: inline CSS, no script, no
        // external request — a report must render identically offline.
        assert!(!h.contains("<script"));
        assert!(!h.contains("http://") && !h.contains("https://"));
        assert!(!h.contains("src="));
        assert_eq!(h, build_html(&sample(false)), "must be deterministic");
    }

    #[test]
    fn german_numbers_and_the_shared_percent_rule() {
        let h = build_html(&sample(false));
        assert!(h.contains("60.000 €"), "German thousands grouping");
        assert!(h.contains("37.795 €"));
        // ⚠️ Percent formatting goes through report_style::pct — the shared
        // rule that once produced "11,8 %" beside "11.8 %" in one document.
        assert!(h.contains("37,0 %"));
        assert!(!h.contains("37.0 %"), "US decimal point leaked into a percent");
    }

    #[test]
    fn a_zero_row_is_stated_not_dropped() {
        // Soli 0 € under the 2025 tariff is an ANSWER (below the threshold),
        // not noise — the row must appear with an explicit zero.
        let h = build_html(&sample(false));
        assert!(h.contains("Soli"));
        assert!(h.contains("0 €"));
    }

    #[test]
    fn name_cells_sit_inside_table_cells() {
        // ⚠️ Sight-check regression: `name_cell` yields a <span>; without a
        // wrapping <td> the browser hoists it OUT of the table and the report
        // shows labels above the table and nameless amount rows.
        let h = build_html(&sample(false));
        assert!(h.contains(r#"<tr><td><span class="rp-name">"#));
        assert!(!h.contains(r#"<tr><span"#), "span directly  under <tr> gets hoisted");
    }

    #[test]
    fn every_embedded_name_is_escaped() {
        let h = build_html(&sample(true));
        assert!(h.contains("&lt;&amp; Co&gt;"), "assumptions must be escaped");
        assert!(!h.contains("<& Co>"));
    }

    #[test]
    fn the_mode_changes_wording_never_math() {
        let e = build_html(&sample(false));
        let s = build_html(&sample(true));
        assert!(e.contains("Angestellt") && e.contains("Brutto / Jahr"));
        assert!(s.contains("Unternehmer") && s.contains("Gewinn / Jahr"));
        // Identical numbers in, identical numbers out — the mode is a label.
        for n in ["60.000 €", "37.795 €", "37,0 %"] {
            assert!(e.contains(n) && s.contains(n), "{n}");
        }
    }

    #[test]
    #[ignore = "sight check: writes the reports to $IR_DUMP_DIR"]
    fn dump_for_a_sight_check() {
        let dir = std::env::var("IR_DUMP_DIR").expect("set IR_DUMP_DIR");
        std::fs::write(
            std::path::Path::new(&dir).join("bruno-employee.html"),
            build_html(&BrunoReport {
                assumptions: "Steuerklasse I · Nordrhein-Westfalen · kinderlos · keine Kirchensteuer · GKV +2,45 %".into(),
                ..sample(false)
            }),
        )
        .unwrap();
        let mut s = sample(true);
        s.assumptions =
            "Freiberufler (keine GewSt) · GKV freiwillig ermäßigt · Grundtarif · Nordrhein-Westfalen · kinderlos · keine Kirchensteuer".into();
        s.net_year = 37713.0;
        s.net_month = 3142.8;
        s.taxes = vec![
            BrunoLine { label: "Einkommensteuer".into(), value: 9897.0 },
            BrunoLine { label: "Soli".into(), value: 0.0 },
        ];
        s.social = vec![
            BrunoLine { label: "Krankenversicherung".into(), value: 9870.0 },
            BrunoLine { label: "Pflegeversicherung".into(), value: 2520.0 },
        ];
        std::fs::write(std::path::Path::new(&dir).join("bruno-self.html"), build_html(&s)).unwrap();
    }
}
