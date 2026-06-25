//! First-run seeding of curated default snippets.
//!
//! Bundles a JSON document of 27 hand-written AI-prompt templates
//! (`ai_prompts.json`) directly into the binary via `include_str!`.
//! On first launch we check the `seed.default_snippets_v1` flag in the
//! settings table — if false, we run the existing JSON-import pipeline
//! against the embedded document and flip the flag so we don't re-import
//! on every launch (which would resurrect snippets the user deleted).
//!
//! The same payload is exposed via `restore_default_prompts()` for the
//! Snippets-tab "Restore defaults" button. That path is intentional, not
//! idempotent — the user is explicitly asking for a reset.

use anyhow::Result;

use crate::db::DbHandle;
use crate::settings;
use crate::snippets::{self, ImportResult};

/// Bundled default-snippet JSON. Edit
/// [`core/rust-lib/src/seed/ai_prompts.json`] to change the seed.
pub const DEFAULT_PROMPTS_JSON: &str = include_str!("seed/ai_prompts.json");

/// Bundled Material-Design colour snippets (`shortcut → hex`, no `#`): typing
/// e.g. `reda700` + the expander hotkey writes `D50000`. Generated from a
/// TextExpander export. Edit [`core/rust-lib/src/seed/material_colors.json`].
pub const MATERIAL_COLORS_JSON: &str = include_str!("seed/material_colors.json");

/// Settings key tracking whether the v1 default snippets have been
/// seeded into this database. Bump the suffix (`_v2`, `_v3`, …) only if
/// we want everyone to receive a fresh batch — but think hard before
/// doing that, because users will have edited / deleted entries from v1.
pub const KEY_SEEDED: &str = "seed.default_snippets_v1";

/// Separate flag for the Material-colour pack so it can be rolled out to
/// **existing** databases (whose AI-prompt flag is already set) on the next
/// launch, without re-running the prompt seed.
pub const KEY_SEEDED_COLORS: &str = "seed.material_colors_v1";

/// Seed the bundled default snippets if we haven't yet — the curated AI
/// prompts and the Material-Design colour pack, each behind its own flag so a
/// new pack reaches existing installs exactly once. Idempotent per pack.
pub fn maybe_seed_defaults(db: &DbHandle) -> Result<()> {
    if !settings::get_bool(db, KEY_SEEDED, false)? {
        import_pack(db, "AI prompts", DEFAULT_PROMPTS_JSON);
        settings::set(db, KEY_SEEDED, "true")?;
    }
    if !settings::get_bool(db, KEY_SEEDED_COLORS, false)? {
        import_pack(db, "Material colours", MATERIAL_COLORS_JSON);
        settings::set(db, KEY_SEEDED_COLORS, "true")?;
    }
    Ok(())
}

fn import_pack(db: &DbHandle, label: &str, json: &str) {
    match snippets::import_from_json(db, json) {
        Ok(r) => {
            tracing::info!(
                "seeded {label}: {} imported, {} skipped, {} errors",
                r.imported,
                r.skipped,
                r.errors.len()
            );
            for e in &r.errors {
                tracing::warn!("seed error ({label}): {e}");
            }
        }
        Err(e) => tracing::warn!("seed {label} failed: {e:#}"),
    }
}

/// Re-import the default prompts on demand, regardless of the seed
/// flag. Used by the Snippets-tab "Restore defaults" button. Existing
/// snippets sharing an abbreviation get overwritten (the import path
/// already does upsert-by-abbreviation); user-added snippets with
/// distinct abbreviations are untouched.
pub fn restore_defaults(db: &DbHandle) -> Result<ImportResult> {
    let mut result = snippets::import_from_json(db, DEFAULT_PROMPTS_JSON)?;
    // Also restore the Material-colour pack so one button brings back all
    // bundled defaults.
    let colors = snippets::import_from_json(db, MATERIAL_COLORS_JSON)?;
    result.imported += colors.imported;
    result.skipped += colors.skipped;
    result.errors.extend(colors.errors);
    // Keep both flags set so a future first-run check doesn't re-run this
    // session.
    let _ = settings::set(db, KEY_SEEDED, "true");
    let _ = settings::set(db, KEY_SEEDED_COLORS, "true");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        snippets::init_table(&db).unwrap();
        settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn embedded_json_parses_and_has_25_prompts() {
        let v: Vec<serde_json::Value> = serde_json::from_str(DEFAULT_PROMPTS_JSON).unwrap();
        // Sanity: we shipped exactly 27 curated prompts. If you change the
        // count deliberately, update this test.
        assert_eq!(v.len(), 27, "expected 27 default prompts");
        for entry in &v {
            assert!(entry.get("abbreviation").is_some());
            assert!(entry.get("title").is_some());
            assert!(entry.get("body").is_some());
            let abbr = entry["abbreviation"].as_str().unwrap();
            assert!(
                abbr.starts_with("ai"),
                "default abbreviation {abbr:?} should start with 'ai'"
            );
        }
    }

    #[test]
    fn maybe_seed_inserts_on_first_run_and_skips_after() {
        let db = test_db();
        assert_eq!(snippets::list_all(&db).unwrap().len(), 0);

        maybe_seed_defaults(&db).unwrap();
        let count_after_first = snippets::list_all(&db).unwrap().len();
        assert!(count_after_first >= 25);

        // Manually delete one — second run should NOT bring it back.
        let s = snippets::list_all(&db).unwrap();
        let id = s[0].id;
        snippets::delete(&db, id).unwrap();
        let count_after_delete = snippets::list_all(&db).unwrap().len();
        assert_eq!(count_after_delete, count_after_first - 1);

        maybe_seed_defaults(&db).unwrap();
        let count_after_second_seed = snippets::list_all(&db).unwrap().len();
        assert_eq!(
            count_after_second_seed, count_after_delete,
            "second seed must not revive deleted snippets"
        );
    }

    #[test]
    fn restore_defaults_re_imports_explicitly() {
        let db = test_db();
        maybe_seed_defaults(&db).unwrap();
        let s = snippets::list_all(&db).unwrap();
        let id = s[0].id;
        snippets::delete(&db, id).unwrap();

        let count_after_delete = snippets::list_all(&db).unwrap().len();
        let result = restore_defaults(&db).unwrap();
        // Restore re-imports — the deleted snippet is back as a new row.
        let count_after_restore = snippets::list_all(&db).unwrap().len();
        assert!(
            count_after_restore > count_after_delete,
            "restore_defaults must re-add deleted snippets ({} -> {})",
            count_after_delete,
            count_after_restore
        );
        assert!(result.imported >= 25);
    }

    #[test]
    fn embedded_json_has_unique_abbreviations() {
        // Snippets are upserted by abbreviation; duplicates in the seed file
        // would cause earlier-position prompts to be overwritten by later ones
        // *silently*. The user would never see the first occurrence.
        let v: Vec<serde_json::Value> = serde_json::from_str(DEFAULT_PROMPTS_JSON).unwrap();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for entry in &v {
            let abbr = entry["abbreviation"].as_str().unwrap().to_string();
            assert!(
                seen.insert(abbr.clone()),
                "duplicate abbreviation {abbr:?} in seed JSON — earlier prompt would be silently overwritten",
            );
        }
    }

    #[test]
    fn embedded_json_all_bodies_are_non_empty() {
        // Empty-bodied prompts would expand to nothing, which is worse than
        // just not shipping them. Catch the regression at compile-test time.
        let v: Vec<serde_json::Value> = serde_json::from_str(DEFAULT_PROMPTS_JSON).unwrap();
        for entry in &v {
            let abbr = entry["abbreviation"].as_str().unwrap();
            let body = entry["body"].as_str().unwrap();
            assert!(!body.is_empty(), "empty body for prompt {abbr:?}");
            assert!(
                body.trim().len() > 10,
                "suspiciously short body for {abbr:?}: {body:?}",
            );
        }
    }

    #[test]
    fn embedded_json_all_titles_are_non_empty() {
        let v: Vec<serde_json::Value> = serde_json::from_str(DEFAULT_PROMPTS_JSON).unwrap();
        for entry in &v {
            let abbr = entry["abbreviation"].as_str().unwrap();
            let title = entry["title"].as_str().unwrap();
            assert!(!title.trim().is_empty(), "empty title for {abbr:?}");
        }
    }

    #[test]
    fn embedded_json_all_abbreviations_are_ai_prefixed() {
        // The README + docs promise every default prompt has an `ai*` prefix
        // so users can scope searches with a single character.
        let v: Vec<serde_json::Value> = serde_json::from_str(DEFAULT_PROMPTS_JSON).unwrap();
        for entry in &v {
            let abbr = entry["abbreviation"].as_str().unwrap();
            assert!(
                abbr.starts_with("ai"),
                "abbreviation {abbr:?} is missing the `ai` prefix",
            );
            assert!(
                abbr.len() > 2,
                "abbreviation {abbr:?} is just the prefix — needs more characters",
            );
        }
    }

    #[test]
    fn material_colors_json_is_valid() {
        let v: Vec<serde_json::Value> = serde_json::from_str(MATERIAL_COLORS_JSON).unwrap();
        assert_eq!(v.len(), 255, "expected 255 Material colour snippets");
        let mut seen = std::collections::HashSet::new();
        let hex = regex_lite_hex();
        for entry in &v {
            let abbr = entry["abbreviation"].as_str().unwrap();
            let body = entry["body"].as_str().unwrap();
            let title = entry["title"].as_str().unwrap();
            assert!(seen.insert(abbr.to_string()), "duplicate shortcut {abbr:?}");
            assert_eq!(abbr, abbr.to_lowercase(), "{abbr:?} not lowercase");
            assert!(!abbr.contains(char::is_whitespace), "{abbr:?} has whitespace");
            assert!(!title.trim().is_empty(), "empty title for {abbr:?}");
            assert!(hex(body), "body {body:?} for {abbr:?} is not 6 hex digits (no #)");
        }
    }

    /// Tiny `^[0-9A-F]{6}$` check without pulling in a regex dep.
    fn regex_lite_hex() -> impl Fn(&str) -> bool {
        |s: &str| s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    }

    #[test]
    fn maybe_seed_includes_material_colors() {
        let db = test_db();
        maybe_seed_defaults(&db).unwrap();
        let all = snippets::list_all(&db).unwrap();
        // 27 AI prompts + 255 colours.
        assert!(all.len() >= 255 + 27, "expected prompts + colours, got {}", all.len());
        // A known colour shortcut round-trips to its hex.
        let reda700 = all.iter().find(|s| s.abbreviation == "reda700").expect("reda700 seeded");
        assert_eq!(reda700.body, "D50000");
    }

    #[test]
    fn embedded_json_abbreviations_are_lowercase_no_whitespace() {
        // The expander matches abbreviations character-by-character — uppercase
        // or whitespace in the abbreviation would never trigger.
        let v: Vec<serde_json::Value> = serde_json::from_str(DEFAULT_PROMPTS_JSON).unwrap();
        for entry in &v {
            let abbr = entry["abbreviation"].as_str().unwrap();
            assert_eq!(
                abbr,
                abbr.to_lowercase(),
                "abbreviation {abbr:?} contains uppercase — won't match user input",
            );
            assert!(
                !abbr.contains(char::is_whitespace),
                "abbreviation {abbr:?} contains whitespace",
            );
        }
    }
}
