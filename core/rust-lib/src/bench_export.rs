//! HTML for a benchmark run and for a comparison of several runs.
//!
//! Uses the shared [`crate::report_style`], so a benchmark report looks like
//! `loc`, `repo`, `pagespeed` and the timesheet: light, print-first, A4,
//! hairlines instead of boxes, tabular figures. One renderer feeds HTML **and**
//! PDF — a second one would drift (see `docs/reports.md`).

use crate::bench::{BenchRun, MachineInfo, Section};
use crate::loc_export::esc;
use crate::report_style as rs;

/// Shown wherever a value could not be READ. Mirrors `lib/bench.ts::UNKNOWN`;
/// never a placeholder number.
pub const UNKNOWN: &str = "nicht verfügbar";

/// ⚠️ Measured: two consecutive runs on the same idle machine differed by up
/// to 5 % per workload. The report says so, so nobody reads a 2 % gap as a
/// result.
pub const NOISE_FLOOR_PCT: f64 = 5.0;

fn opt(v: &Option<String>) -> String {
    v.as_deref().map(esc).unwrap_or_else(|| UNKNOWN.into())
}

fn opt_num<T: std::fmt::Display>(v: &Option<T>) -> String {
    v.as_ref().map(|x| x.to_string()).unwrap_or_else(|| UNKNOWN.into())
}

fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, U[i]).replace('.', ",")
}

fn machine_label(m: &MachineInfo) -> String {
    m.device_model
        .clone()
        .or_else(|| m.cpu_brand.clone())
        .or_else(|| m.host_name.clone())
        .unwrap_or_else(|| UNKNOWN.into())
}

/// `macOS 26.6.2` — or as much of it as could be read.
///
/// ⚠️ Does NOT blindly join the two fields. On macOS `long_os_version()`
/// already ends in the version, and appending `os_version` produced
/// "MacOS 26.6.2  26.6.2" in a rendered report. Mirrored in `lib/bench.ts`.
pub fn os_label(m: &MachineInfo) -> String {
    match (m.os_name.as_deref(), m.os_version.as_deref()) {
        (None, None) => UNKNOWN.into(),
        (Some(n), None) => n.trim().into(),
        (None, Some(v)) => v.trim().into(),
        (Some(n), Some(v)) if n.trim().ends_with(v.trim()) => n.trim().into(),
        (Some(n), Some(v)) => format!("{} {}", n.trim(), v.trim()),
    }
}

/// The system table — every row read, `nicht verfügbar` where it was not.
fn bt(k: &str) -> String { crate::i18n::t(k) }

fn machine_rows(m: &MachineInfo) -> Vec<(String, String)> {
    vec![
        (bt("bench.m.device"), opt(&m.device_model)),
        (bt("bench.m.os"), esc(&os_label(m))),
        (bt("bench.m.kernel"), opt(&m.kernel)),
        (bt("bench.m.arch"), opt(&m.arch)),
        (bt("bench.m.cpu"), opt(&m.cpu_brand)),
        (bt("bench.m.cores_phys"), opt_num(&m.physical_cores)),
        (bt("bench.m.cores_log"), opt_num(&m.logical_cores)),
        (
            bt("bench.m.memory"),
            m.mem_total_bytes.map(human_bytes).unwrap_or_else(|| UNKNOWN.into()),
        ),
        (bt("bench.m.host"), opt(&m.host_name)),
    ]
}

fn section_table(s: &Section, title: &str) -> String {
    let max = s.workloads.iter().map(|w| w.score).max().unwrap_or(1).max(1);
    let rows: String = s
        .workloads
        .iter()
        .map(|w| {
            let share = f64::from(w.score) / f64::from(max);
            format!(
                "<tr><td>{}</td><td class=\"rp-text rp-dim\">{}</td>\
                 <td class=\"rp-num\">{}</td><td class=\"rp-num\">{}</td></tr>",
                rs::name_cell(rs::series_color(&w.id), &esc(&w.name), share),
                esc(&w.unit),
                format!("{:.2}", w.rate).replace('.', ","),
                w.score
            )
        })
        .collect();
    format!(
        "<section><h2>{title}</h2>\
         <p class=\"rp-lede\">{lede}</p>\
         <table><thead><tr><th>{c_disc}</th><th class=\"rp-text\">{c_unit}</th>\
         <th class=\"rp-num\">{c_meas}</th><th class=\"rp-num\">{c_pts}</th></tr></thead>\
         <tbody>{rows}</tbody></table></section>",
        lede = crate::i18n::tf("bench.lede", &[("score", &s.score.to_string())]),
        c_disc = bt("bench.c.discipline"), c_unit = bt("bench.c.unit"), c_meas = bt("bench.c.measured"), c_pts = bt("bench.c.points"),
    )
}

/// The report for ONE run.
pub fn build_html(run: &BenchRun) -> String {
    let m = &run.machine;
    let (sc, mc) = (bt("bench.single"), bt("bench.multi"));
    let stats = rs::stats(&[
        rs::Stat { label: &sc, value: run.single.score.to_string(), unit: None },
        rs::Stat { label: &mc, value: run.multi.score.to_string(), unit: None },
        rs::Stat { label: &bt("bench.threads"), value: run.threads.to_string(), unit: None },
        rs::Stat { label: &bt("bench.duration"), value: format!("{:.1}", run.duration_s).replace('.', ","), unit: Some(" s") },
    ]);
    let mrows: String = machine_rows(m)
        .into_iter()
        .map(|(k, v)| format!("<tr><td>{k}</td><td class=\"rp-text\">{v}</td></tr>"))
        .collect();
    let body = format!(
        "{stats}\
         <section><h2>{m_head}</h2><table><tbody>{mrows}</tbody></table></section>\
         {single}{multi}",
        single = section_table(&run.single, &sc),
        multi = section_table(&run.multi, &mc),
        m_head = bt("bench.machine"),
    );
    let foot = crate::i18n::tf("bench.foot", &[("baseline", &esc(&run.baseline_machine)), ("noise", &format!("{:.0}", NOISE_FLOOR_PCT)), ("version", &esc(&run.app_version))]);
    rs::shell(
        &bt("bench.title"),
        &esc(&machine_label(m)),
        &esc(&os_label(m)),
        &body,
        &foot,
    )
}

/// The comparison of several runs — possibly from different machines and OSes.
pub fn build_compare_html(runs: &[BenchRun]) -> String {
    if runs.is_empty() {
        return rs::shell(&bt("bench.compare_title"), &bt("bench.no_runs"), "", &crate::i18n::t("bench.nothing"), "");
    }
    let head_cells: String = runs
        .iter()
        .map(|r| format!("<th class=\"rp-num\">{}</th>", esc(&machine_label(&r.machine))))
        .collect();

    // Machine table: one column per run, so different OSes sit side by side.
    let keys = machine_rows(&runs[0].machine);
    let mrows: String = (0..keys.len())
        .map(|i| {
            let label = &keys[i].0;
            let cells: String = runs
                .iter()
                .map(|r| format!("<td class=\"rp-text\">{}</td>", machine_rows(&r.machine)[i].1))
                .collect();
            format!("<tr><td>{label}</td>{cells}</tr>")
        })
        .collect();

    let section = |pick: fn(&BenchRun) -> &Section, title: &str| -> String {
        // ⚠️ Joined by workload ID, never by position: a run from another
        // version can carry a different set, and lining those up by index
        // would compare SHA-256 against a prime sieve.
        let mut order: Vec<String> = Vec::new();
        for r in runs {
            for w in &pick(r).workloads {
                if !order.contains(&w.id) {
                    order.push(w.id.clone());
                }
            }
        }
        let rows: String = order
            .iter()
            .map(|id| {
                let base = pick(&runs[0]).workloads.iter().find(|w| &w.id == id);
                let name = runs
                    .iter()
                    .find_map(|r| pick(r).workloads.iter().find(|w| &w.id == id))
                    .map(|w| w.name.clone())
                    .unwrap_or_else(|| id.clone());
                let cells: String = runs
                    .iter()
                    .enumerate()
                    .map(|(i, r)| match pick(r).workloads.iter().find(|w| &w.id == id) {
                        None => "<td class=\"rp-num rp-dim\">—</td>".to_string(),
                        Some(w) => {
                            let d = match (i, base) {
                                (0, _) | (_, None) => String::new(),
                                (_, Some(b)) if b.score > 0 => {
                                    let pct = (f64::from(w.score) - f64::from(b.score))
                                        / f64::from(b.score)
                                        * 100.0;
                                    let noise = if pct.abs() < NOISE_FLOOR_PCT { " rp-dim" } else { "" };
                                    format!(
                                        "<span class=\"bx-d{noise}\">{}{:.0} %</span>",
                                        if pct >= 0.0 { "+" } else { "−" },
                                        pct.abs()
                                    )
                                }
                                _ => String::new(),
                            };
                            format!("<td class=\"rp-num\">{} {d}</td>", w.score)
                        }
                    })
                    .collect();
                format!("<tr><td>{}</td>{cells}</tr>", esc(&name))
            })
            .collect();
        let totals: String = runs
            .iter()
            .map(|r| format!("<td class=\"rp-num\">{}</td>", pick(r).score))
            .collect();
        format!(
            "<section><h2>{title}</h2><table><thead><tr><th>{c}</th>{head_cells}</tr></thead>\
             <tbody>{rows}</tbody><tfoot><tr><td>{tot}</td>{totals}</tr></tfoot></table></section>",
            c = bt("bench.c.discipline"), tot = bt("bench.c.total")
        )
    };

    let body = format!(
        "<section><h2>{machines}</h2><table><thead><tr><th>{feat}</th>{head_cells}</tr></thead>\
         <tbody>{mrows}</tbody></table></section>{s}{m}",
        machines = bt("bench.machines"), feat = bt("bench.feature"),
        s = section(|r| &r.single, &bt("bench.single")),
        m = section(|r| &r.multi, &bt("bench.multi")),
    );
    let foot = crate::i18n::tf("bench.compare_foot", &[("noise", &format!("{:.0}", NOISE_FLOOR_PCT))]);
    let doc = rs::shell(
        &bt("bench.compare_title"),
        &crate::i18n::tf("bench.n_runs", &[("n", &runs.len().to_string())]),
        "",
        &body,
        &foot,
    );
    doc.replace("</style>", ".bx-d { margin-left: 6px; font-weight: 500; font-size: 11px }\n</style>")
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
    use crate::bench::{Section, WorkloadResult};

    fn wl(id: &str, score: u32) -> WorkloadResult {
        WorkloadResult {
            id: id.into(), name: format!("Disziplin {id}"), unit: "MB/s".into(),
            rate: f64::from(score) / 10.0, score, iterations: 3, seconds: 0.6,
        }
    }

    fn run(id: &str, ids: &[(&str, u32)], m: MachineInfo) -> BenchRun {
        BenchRun {
            schema: 1, id: id.into(), finished_at_ms: 0, duration_s: 8.6,
            app_version: "0.150.0".into(), baseline_machine: "Referenz".into(),
            machine: m, threads: 10,
            single: Section { score: 1000, workloads: ids.iter().map(|(i, s)| wl(i, *s)).collect() },
            multi: Section { score: 5000, workloads: ids.iter().map(|(i, s)| wl(i, s * 5)).collect() },
        }
    }

    fn full() -> MachineInfo {
        MachineInfo {
            os_name: Some("macOS".into()), os_version: Some("26.6.2".into()),
            kernel: Some("25.6.0".into()), arch: Some("aarch64".into()),
            device_model: Some("MacBookPro18,1".into()), host_name: Some("host".into()),
            cpu_brand: Some("Apple M1 Pro".into()), physical_cores: Some(10),
            logical_cores: Some(10), mem_total_bytes: Some(34_359_738_368),
        }
    }

    /// Offline sight check: writes a real run's report and a comparison so
    /// they can be opened and LOOKED at. Test-green and looks-right are
    /// different claims. `IR_DUMP_DIR=... cargo test -p inspector-rust-core --lib bench_export::tests::dump -- --ignored`
    #[test]
    #[ignore]
    fn dump_for_a_sight_check() {
        let dir = std::path::PathBuf::from(
            std::env::var("IR_DUMP_DIR").unwrap_or_else(|_| "/tmp".into()),
        );
        // The FIRST run is a real measurement on this machine.
        let run = crate::bench::run(|_, _, _| {});
        std::fs::write(dir.join("bench-report.html"), build_html(&run)).unwrap();
        // The second is that run SCALED and relabelled — a layout fixture so
        // the comparison can be looked at, never a measurement of a ThinkPad.
        // It exists only inside this ignored test and reaches no export.
        let mut other = run.clone();
        other.id = "vergleich".into();
        other.machine.device_model = Some("ThinkPad X1".into());
        other.machine.os_name = Some("Ubuntu".into());
        other.machine.os_version = Some("24.04".into());
        other.machine.cpu_brand = Some("Intel Core i7-1365U".into());
        other.machine.kernel = Some("6.8.0-40-generic".into());
        for w in other.single.workloads.iter_mut() {
            w.score = (f64::from(w.score) * 0.78) as u32;
        }
        for w in other.multi.workloads.iter_mut() {
            w.score = (f64::from(w.score) * 1.02) as u32;
        }
        // ⚠️ Recompute the section totals. Scaling the workloads alone left
        // the footer showing the ORIGINAL total next to reduced rows — a
        // fixture that lies makes the sight check worthless.
        other.single.score =
            crate::bench::overall_score(&other.single.workloads.iter().map(|w| w.score).collect::<Vec<_>>());
        other.multi.score =
            crate::bench::overall_score(&other.multi.workloads.iter().map(|w| w.score).collect::<Vec<_>>());
        std::fs::write(dir.join("bench-compare.html"), build_compare_html(&[run, other])).unwrap();
        eprintln!("geschrieben nach {}", dir.display());
    }

    #[test]
    fn the_os_line_never_repeats_the_version() {
        // ⚠️ `long_os_version()` already ends in the version on macOS; joining
        // both fields printed "MacOS 26.6.2  26.6.2" in a real report.
        let mac = MachineInfo {
            os_name: Some("MacOS 26.6.2".into()),
            os_version: Some("26.6.2".into()),
            ..Default::default()
        };
        assert_eq!(os_label(&mac), "MacOS 26.6.2");
        // A name WITHOUT the version still gets it appended.
        let ubu = MachineInfo {
            os_name: Some("Ubuntu".into()),
            os_version: Some("24.04".into()),
            ..Default::default()
        };
        assert_eq!(os_label(&ubu), "Ubuntu 24.04");
        assert_eq!(os_label(&MachineInfo::default()), UNKNOWN);
    }

    #[test]
    fn the_report_is_self_contained() {
        // Same contract as every other export: offline, no script, no request.
        let h = build_html(&run("a", &[("sort", 1000)], full()));
        assert!(h.contains("</html>"));
        assert!(!h.contains("<script"));
        let h = sans_data_uris(&h);
        assert!(!h.contains("http://") && !h.contains("https://"));
        // ⚠️ The embedded signature legitimately uses src="data:…"; only an
        // EXTERNAL src is a violation.
        assert!(!h.replace("src=\"data:", "").contains("src="));
    }

    #[test]
    fn an_unreadable_detail_says_so_instead_of_showing_a_number() {
        // ⚠️ The whole point of the Option fields: a value that could not be
        // READ must never appear as a plausible-looking figure.
        let h = build_html(&run("a", &[("sort", 1000)], MachineInfo::default()));
        assert!(h.contains(UNKNOWN), "missing values must be named");
        let body = h.split("</style>").nth(1).expect("has a stylesheet");
        assert!(!body.contains(">0<"), "no zero stood in for an unread value");
    }

    #[test]
    fn every_machine_field_reaches_the_report() {
        let h = build_html(&run("a", &[("sort", 1000)], full()));
        for needle in ["MacBookPro18,1", "26.6.2", "aarch64", "Apple M1 Pro", "25.6.0"] {
            assert!(h.contains(needle), "{needle} missing from the report");
        }
    }

    #[test]
    fn the_yardstick_and_the_noise_floor_are_stated() {
        // A score is meaningless without saying what 1000 is, and a delta is
        // misleading without saying how much of it is noise.
        let h = build_html(&run("a", &[("sort", 1000)], full()));
        assert!(h.contains("reference"));
        assert!(h.contains("1000"));
        assert!(h.contains("noise"));
    }

    #[test]
    fn injected_names_are_escaped() {
        let mut m = full();
        m.device_model = Some("<script>alert(1)</script>".into());
        let h = build_html(&run("a", &[("sort", 1000)], m));
        assert!(!h.contains("<script>alert"));
        assert!(h.contains("&lt;script&gt;"));
    }

    #[test]
    fn the_comparison_joins_by_id_and_leaves_a_gap() {
        // ⚠️ By ID, never by position — a run from another version can carry a
        // different set, and lining those up by index would compare SHA-256
        // against a prime sieve.
        let a = run("a", &[("sort", 1000), ("sha256", 1000)], full());
        let b = run("b", &[("sha256", 2000)], full());
        let h = build_compare_html(&[a, b]);
        assert!(h.contains("Disziplin sort"));
        assert!(h.contains("Disziplin sha256"));
        assert!(h.contains("—"), "the missing workload needs a gap, not a zero");
        assert!(h.contains("+100 %"), "the delta against the first run");
    }

    #[test]
    fn a_delta_below_the_noise_floor_is_marked_as_noise() {
        let a = run("a", &[("sort", 1000)], full());
        let b = run("b", &[("sort", 1020)], full()); // +2 %
        let h = build_compare_html(&[a, b]);
        assert!(h.contains("rp-dim"), "a 2 % difference must be dimmed, not asserted");
    }

    #[test]
    fn an_empty_comparison_renders_rather_than_panicking() {
        let h = build_compare_html(&[]);
        assert!(h.contains("</html>"));
    }
}
