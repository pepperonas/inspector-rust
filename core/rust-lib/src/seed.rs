//! First-run seeding of curated default snippets.
//!
//! Bundles a JSON document of ~25 hand-written AI-prompt templates
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

/// Settings key tracking whether the v1 default snippets have been
/// seeded into this database. Bump the suffix (`_v2`, `_v3`, …) only if
/// we want everyone to receive a fresh batch — but think hard before
/// doing that, because users will have edited / deleted entries from v1.
pub const KEY_SEEDED: &str = "seed.default_snippets_v1";

/// Seed the default AI-prompt snippets if we haven't done so yet.
/// Idempotent: re-runs do nothing once the flag is set.
pub fn maybe_seed_defaults(db: &DbHandle) -> Result<()> {
    let already = settings::get_bool(db, KEY_SEEDED, false)?;
    if already {
        return Ok(());
    }

    let result = snippets::import_from_json(db, DEFAULT_PROMPTS_JSON)?;
    tracing::info!(
        "seeded default snippets: {} imported, {} skipped, {} errors",
        result.imported,
        result.skipped,
        result.errors.len()
    );
    if !result.errors.is_empty() {
        for e in &result.errors {
            tracing::warn!("seed error: {e}");
        }
    }

    settings::set(db, KEY_SEEDED, "true")?;
    Ok(())
}

/// Re-import the default prompts on demand, regardless of the seed
/// flag. Used by the Snippets-tab "Restore defaults" button. Existing
/// snippets sharing an abbreviation get overwritten (the import path
/// already does upsert-by-abbreviation); user-added snippets with
/// distinct abbreviations are untouched.
pub fn restore_defaults(db: &DbHandle) -> Result<ImportResult> {
    let result = snippets::import_from_json(db, DEFAULT_PROMPTS_JSON)?;
    // Make sure the flag stays set so a future first-run check doesn't
    // re-run during this session.
    let _ = settings::set(db, KEY_SEEDED, "true");
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
        // Sanity: we shipped exactly 25 curated prompts. If you change the
        // count deliberately, update this test.
        assert_eq!(v.len(), 25, "expected 25 default prompts");
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
