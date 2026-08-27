//! `loc` — lines-of-code statistics for a folder (v0.117.0).
//!
//! Counting engine is the `tokei` crate (v14, MIT) — the de-facto standard
//! LOC counter (~200 languages with real per-language syntax knowledge:
//! block comments, strings, nested comments, literate languages, embedded
//! languages like JS inside HTML). Hand-rolling comment parsing per language
//! would be exactly the correctness trap this command must avoid.
//!
//! Semantics mirrored from tokei's own CLI (and the IntelliJ Statistic
//! plugin's columns): per language **files / code / comments / blanks**,
//! where comments INCLUDE documentation comments — additionally
//! `treat_doc_strings_as_comments` is enabled so Python-style docstrings
//! count as comments rather than code (a separate "docs" column across all
//! languages is not reliably possible in any counter, and the IntelliJ
//! plugin doesn't split it either).
//!
//! Ignore behaviour: by default tokei respects `.gitignore`/`.ignore` and
//! skips hidden files — without that, any JS project's `node_modules` dwarfs
//! the user's own code and the statistic says nothing. The panel offers a
//! toggle (`respect_ignores: false` → `no_ignore` + `hidden`), so vendored
//! trees CAN be counted deliberately.
//!
//! House style: `count()` is the thin impure shell (tokei walks the file
//! system); the aggregation (`aggregate` — sorting, percentages, totals) is
//! pure and unit-tested, plus a REAL end-to-end fixture test: a temp dir
//! with hand-written Rust + Python files whose exact code/comment/blank
//! counts are asserted (tokei is deterministic and offline, so this is a
//! legitimate hermetic test, not a network test).

use serde::{Deserialize, Serialize};

/// One language row of the report, ready for the panel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocLanguage {
    pub name: String,
    pub files: usize,
    pub code: usize,
    pub comments: usize,
    pub blanks: usize,
    /// This language's share of all CODE lines, 0..100 (chart + bar width).
    pub code_pct: f64,
}

/// The whole report. `total_lines` = code + comments + blanks (the "lines"
/// column of the IntelliJ plugin).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocReport {
    /// Display label — the folder name (or "N Auswahlen" for a multi-select).
    pub root_label: String,
    pub paths: Vec<String>,
    pub respected_ignores: bool,
    /// Sorted by code descending.
    pub languages: Vec<LocLanguage>,
    pub total_files: usize,
    pub total_code: usize,
    pub total_comments: usize,
    pub total_blanks: usize,
    pub total_lines: usize,
    /// tokei flagged parse problems somewhere — surface honestly, never hide.
    pub inaccurate: bool,
    /// Immediate subdirectory names of the counted folder, sorted — what the
    /// panel offers to descend into. Empty for a multi-path count (there is no
    /// single "current folder" then) or when the path is a file.
    #[serde(default)]
    pub subdirs: Vec<String>,
}

/// Raw per-language numbers before aggregation (the pure seam — the impure
/// shell maps tokei's types onto this, tests construct it directly).
#[derive(Clone, Debug)]
pub struct RawLang {
    pub name: String,
    pub files: usize,
    pub code: usize,
    pub comments: usize,
    pub blanks: usize,
    pub inaccurate: bool,
}

/// Pure: rows → sorted report body. Percentages are of CODE lines (that is
/// what "how much of the code is which language" means — GitHub's language
/// bar does the same); a zero-code total yields 0 % everywhere rather than
/// NaN. Sort: code desc, then name (stable for equal counts).
pub fn aggregate(
    mut rows: Vec<RawLang>,
    root_label: String,
    paths: Vec<String>,
    respected_ignores: bool,
) -> LocReport {
    rows.retain(|r| r.code + r.comments + r.blanks > 0 || r.files > 0);
    rows.sort_by(|a, b| b.code.cmp(&a.code).then_with(|| a.name.cmp(&b.name)));
    let total_code: usize = rows.iter().map(|r| r.code).sum();
    let total_comments: usize = rows.iter().map(|r| r.comments).sum();
    let total_blanks: usize = rows.iter().map(|r| r.blanks).sum();
    let total_files: usize = rows.iter().map(|r| r.files).sum();
    let inaccurate = rows.iter().any(|r| r.inaccurate);
    let languages = rows
        .into_iter()
        .map(|r| LocLanguage {
            code_pct: if total_code > 0 {
                (r.code as f64 / total_code as f64) * 100.0
            } else {
                0.0
            },
            name: r.name,
            files: r.files,
            code: r.code,
            comments: r.comments,
            blanks: r.blanks,
        })
        .collect();
    LocReport {
        root_label,
        paths,
        respected_ignores,
        languages,
        total_files,
        total_code,
        total_comments,
        total_blanks,
        total_lines: total_code + total_comments + total_blanks,
        inaccurate,
        // Filled by `count` for a single-folder run; `aggregate` is pure and
        // must not touch the filesystem.
        subdirs: Vec::new(),
    }
}

/// Display label for the selection: one path → its file name, several → "N
/// Pfade". Pure.
pub fn root_label_for(paths: &[String]) -> String {
    match paths {
        [] => String::new(),
        [one] => std::path::Path::new(one)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| one.clone()),
        many => format!("{} Pfade", many.len()),
    }
}

/// Impure shell: run tokei over `paths`. Rejects empty input and paths that
/// don't exist (a stale Finder selection must error clearly, not report an
/// empty-but-plausible zero count).
pub fn count(paths: &[String], respect_ignores: bool) -> Result<LocReport, String> {
    if paths.is_empty() {
        return Err("loc.no_paths".into());
    }
    for p in paths {
        if !std::path::Path::new(p).exists() {
            return Err(format!("path does not exist: {p}"));
        }
    }
    let config = tokei::Config {
        // Python docstrings etc. → comments ("Kommentare und Dokumentation").
        treat_doc_strings_as_comments: Some(true),
        no_ignore: (!respect_ignores).then_some(true),
        hidden: (!respect_ignores).then_some(true),
        ..tokei::Config::default()
    };
    let mut languages = tokei::Languages::new();
    languages.get_statistics(paths, &[], &config);
    let rows = languages
        .iter()
        .map(|(ty, lang)| {
            // `summarise()` folds embedded children (JS inside HTML …) into
            // the parent's line counts; `reports.len()` stays the number of
            // actual FILES of this language (children live inside the same
            // files), so files never double-count.
            let sum = lang.summarise();
            RawLang {
                name: ty.name().to_string(),
                files: lang.reports.len(),
                code: sum.code,
                comments: sum.comments,
                blanks: sum.blanks,
                inaccurate: sum.inaccurate,
            }
        })
        .collect();
    let mut report = aggregate(
        rows,
        root_label_for(paths),
        paths.to_vec(),
        respect_ignores,
    );
    // Navigation only makes sense for a single folder — with several paths
    // counted at once there is no "current directory" to descend from.
    if let [only] = paths {
        report.subdirs = subdirs(std::path::Path::new(only), !respect_ignores);
    }
    Ok(report)
}

/// The immediate subdirectory names of `dir`, sorted case-insensitively.
///
/// Hidden folders follow the same rule as the count itself: with ignores
/// respected (the default) they stay out, so the list matches what was
/// actually counted. Symlinked directories are skipped — descending one would
/// leave the tree the user thinks they are in.
pub fn subdirs(dir: &std::path::Path, include_hidden: bool) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .filter(|e| e.metadata().map(|m| m.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| include_hidden || !n.starts_with('.'))
        .collect();
    out.sort_by_key(|a| a.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdirs_lists_folders_only_sorted_and_honours_the_hidden_rule() {
        let root = std::env::temp_dir().join(format!("ir-loc-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["src", "Assets", ".git", "node_modules"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("README.md"), "x").unwrap();

        // Ignores respected → hidden folders stay out, so the list matches
        // what was actually counted. Case-insensitive order, files excluded.
        assert_eq!(
            subdirs(&root, false),
            vec!["Assets".to_string(), "node_modules".into(), "src".into()]
        );
        // …and with hidden included, `.git` appears.
        assert!(subdirs(&root, true).contains(&".git".to_string()));
        // A path that isn't a directory yields nothing rather than erroring.
        assert!(subdirs(&root.join("README.md"), true).is_empty());
        assert!(subdirs(&root.join("gibtsnicht"), true).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Pure aggregation ─────────────────────────────────────────────

    fn raw(name: &str, files: usize, code: usize, comments: usize, blanks: usize) -> RawLang {
        RawLang { name: name.into(), files, code, comments, blanks, inaccurate: false }
    }

    #[test]
    fn aggregate_sorts_by_code_and_computes_percentages_and_totals() {
        let r = aggregate(
            vec![raw("Python", 1, 25, 5, 5), raw("Rust", 2, 75, 10, 10)],
            "proj".into(),
            vec!["/p".into()],
            true,
        );
        assert_eq!(r.languages[0].name, "Rust"); // most code first
        assert_eq!(r.languages[1].name, "Python");
        assert!((r.languages[0].code_pct - 75.0).abs() < 1e-9);
        assert!((r.languages[1].code_pct - 25.0).abs() < 1e-9);
        assert_eq!(r.total_files, 3);
        assert_eq!(r.total_code, 100);
        assert_eq!(r.total_comments, 15);
        assert_eq!(r.total_blanks, 15);
        assert_eq!(r.total_lines, 130);
        assert!(!r.inaccurate);
    }

    #[test]
    fn aggregate_is_nan_free_on_zero_code_and_ties_sort_by_name() {
        // Comment-only content (a docs folder): total code 0 → 0 %, not NaN.
        let r = aggregate(
            vec![raw("Markdown", 1, 0, 40, 3), raw("JSON", 1, 0, 0, 1)],
            "docs".into(),
            vec![],
            true,
        );
        assert!(r.languages.iter().all(|l| l.code_pct == 0.0));
        // Equal code (0) → alphabetical, deterministic.
        assert_eq!(r.languages[0].name, "JSON");
        assert_eq!(r.languages[1].name, "Markdown");
    }

    #[test]
    fn aggregate_drops_ghost_rows_and_flags_inaccuracy() {
        let mut ghost = raw("Zig", 0, 0, 0, 0);
        ghost.inaccurate = false;
        let mut bad = raw("Rust", 1, 10, 0, 0);
        bad.inaccurate = true;
        let r = aggregate(vec![ghost, bad], "x".into(), vec![], false);
        assert_eq!(r.languages.len(), 1, "empty language rows never render");
        assert!(r.inaccurate, "tokei parse problems must surface, never hide");
        assert!(!r.respected_ignores);
    }

    #[test]
    fn root_label_is_the_folder_name_or_a_count() {
        assert_eq!(root_label_for(&["/Users/x/proj".into()]), "proj");
        assert_eq!(root_label_for(&["/a".into(), "/b".into()]), "2 Pfade");
        assert_eq!(root_label_for(&[]), "");
    }

    // ── The real thing: exact counts on a hand-written fixture ───────
    //
    // This is the "achte drauf, dass die locs korrekt gezählt werden"
    // verification: tokei runs over files whose code/comment/blank counts
    // were counted BY HAND below. Deterministic + offline (tokei walks the
    // temp dir, no network), so it is a legitimate hermetic test.

    #[test]
    fn counts_a_known_fixture_exactly() {
        let base = std::env::temp_dir().join(format!("ir-loc-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // Rust: 4 code, 4 comments (2 //-style + 2 doc ///), 2 blanks.
        std::fs::write(
            base.join("main.rs"),
            "// a comment\n\n/// doc line one\n/// doc line two\nfn main() {\n    // inner comment\n    println!(\"hi\");\n}\n\nstruct S;\n",
        )
        .unwrap();
        // Python with treat_doc_strings_as_comments: docstring lines (3) are
        // COMMENTS, not code: 2 code, 4 comments (1 # + 3 docstring), 1 blank.
        std::fs::write(
            base.join("tool.py"),
            "# hash comment\ndef f():\n    \"\"\"doc\n    string\n    \"\"\"\n\n    return 1\n",
        )
        .unwrap();

        let report = count(&[base.to_string_lossy().into_owned()], true).unwrap();
        let _ = std::fs::remove_dir_all(&base);

        let rust = report.languages.iter().find(|l| l.name == "Rust").expect("Rust row");
        assert_eq!((rust.files, rust.code, rust.comments, rust.blanks), (1, 4, 4, 2));
        let py = report.languages.iter().find(|l| l.name == "Python").expect("Python row");
        assert_eq!((py.files, py.code, py.comments, py.blanks), (1, 2, 4, 1));
        assert_eq!(report.total_files, 2);
        assert_eq!(report.total_lines, rust.code + rust.comments + rust.blanks + py.code + py.comments + py.blanks);
        // Rust has 4 of 6 code lines → ~66.7 %.
        assert!((rust.code_pct - (4.0 / 6.0 * 100.0)).abs() < 1e-6);
    }

    #[test]
    fn gitignore_is_respected_by_default_and_ignorable_on_demand() {
        let base = std::env::temp_dir().join(format!("ir-loc-ignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("node_modules")).unwrap();
        // ⚠️ The `ignore` crate honours .gitignore only INSIDE a git repo
        // (`require_git`) — without a `.git` dir the ignore file is inert and
        // this test failed with node_modules counted. Real projects are git
        // repos, so the fixture fakes one; also documents the user-visible
        // rule: a non-repo folder's .gitignore does NOT filter the count.
        std::fs::create_dir_all(base.join(".git")).unwrap();
        std::fs::write(base.join(".gitignore"), "node_modules/\n").unwrap();
        std::fs::write(base.join("app.js"), "console.log(1);\n").unwrap();
        std::fs::write(base.join("node_modules/dep.js"), "module.exports = 1;\nmodule.exports = 2;\n").unwrap();

        let with = count(&[base.to_string_lossy().into_owned()], true).unwrap();
        let without = count(&[base.to_string_lossy().into_owned()], false).unwrap();
        let _ = std::fs::remove_dir_all(&base);

        let js_with = with.languages.iter().find(|l| l.name == "JavaScript").unwrap();
        assert_eq!(js_with.code, 1, "ignored node_modules must not count by default");
        let js_without = without.languages.iter().find(|l| l.name == "JavaScript").unwrap();
        assert_eq!(js_without.code, 3, "the toggle counts EVERYTHING deliberately");
    }

    #[test]
    fn missing_paths_and_empty_input_error_clearly() {
        assert_eq!(count(&[], true).unwrap_err(), "loc.no_paths");
        let err = count(&["/definitely/not/here-42".into()], true).unwrap_err();
        assert!(err.contains("does not exist"), "stale selections must error, not zero-count: {err}");
    }
}
