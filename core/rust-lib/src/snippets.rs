use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::DbHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub id: i64,
    pub abbreviation: String,
    pub title: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Group/category NAME the snippet belongs to (joined from
    /// `snippet_categories`), or `None` = ungrouped. Serialized by name (not id)
    /// so it stays portable across machines through backup/export.
    #[serde(default)]
    pub category: Option<String>,
}

/// A snippet group. `id` is stable within a DB; `name` is what travels through
/// backup/export. `count` = snippets currently in it (0 for an empty group).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetCategory {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    #[serde(default)]
    pub count: i64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportResult {
    /// Rows successfully written (insert or update by abbreviation).
    pub imported: usize,
    /// Rows that failed validation; details in `errors`.
    pub skipped: usize,
    /// Per-row error messages, prefixed with the index in the input.
    pub errors: Vec<String>,
}

pub fn init_table(db: &DbHandle) -> Result<()> {
    let conn = db.lock();
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS snippets (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            abbreviation TEXT    NOT NULL UNIQUE,
            title        TEXT    NOT NULL DEFAULT '',
            body         TEXT    NOT NULL,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_snippets_abbr ON snippets(abbreviation);
        CREATE TABLE IF NOT EXISTS snippet_categories (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT    NOT NULL UNIQUE,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )?;
    // Lazy migration: add the grouping column to pre-existing snippet tables.
    let _ = conn.execute("ALTER TABLE snippets ADD COLUMN category_id INTEGER", []);
    Ok(())
}

// ── SELECT with the joined category name ─────────────────────────────────────
const SNIPPET_COLS: &str = "s.id, s.abbreviation, s.title, s.body, s.created_at, s.updated_at, c.name";
const SNIPPET_FROM: &str =
    "FROM snippets s LEFT JOIN snippet_categories c ON c.id = s.category_id";

pub fn list_all(db: &DbHandle) -> Result<Vec<Snippet>> {
    let conn = db.lock();
    let sql = format!("SELECT {SNIPPET_COLS} {SNIPPET_FROM} ORDER BY s.abbreviation ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_snippet)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Look up a single snippet by its abbreviation. Returns the case-sensitive
/// match if one exists; otherwise falls back to a case-insensitive match
/// (so the text expander matches `MFG` to a snippet stored as `mfg` if no
/// `MFG` exists). Empty / whitespace input returns `None`.
pub fn find_by_exact_abbreviation(db: &DbHandle, abbr: &str) -> Result<Option<Snippet>> {
    let trimmed = abbr.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let conn = db.lock();
    let exact: Option<Snippet> = conn
        .query_row(
            &format!("SELECT {SNIPPET_COLS} {SNIPPET_FROM} WHERE s.abbreviation = ?1 LIMIT 1"),
            params![trimmed],
            row_to_snippet,
        )
        .optional()?;
    if let Some(s) = exact {
        return Ok(Some(s));
    }

    let ci: Option<Snippet> = conn
        .query_row(
            &format!(
                "SELECT {SNIPPET_COLS} {SNIPPET_FROM} WHERE LOWER(s.abbreviation) = LOWER(?1) LIMIT 1"
            ),
            params![trimmed],
            row_to_snippet,
        )
        .optional()?;
    Ok(ci)
}

/// Fetch a single snippet by primary key. `None` if it was deleted.
pub fn get_by_id(db: &DbHandle, id: i64) -> Result<Option<Snippet>> {
    let conn = db.lock();
    conn.query_row(
        &format!("SELECT {SNIPPET_COLS} {SNIPPET_FROM} WHERE s.id = ?1"),
        params![id],
        row_to_snippet,
    )
    .optional()
    .map_err(Into::into)
}

/// Match abbreviation prefix first, then body/title contains — up to 10 results.
pub fn find_by_query(db: &DbHandle, query: &str) -> Result<Vec<Snippet>> {
    if query.is_empty() {
        return Ok(vec![]);
    }
    let q = query.to_lowercase();
    let prefix = format!("{}%", q);
    let contains = format!("%{}%", q);
    let conn = db.lock();
    let sql = format!(
        r#"SELECT {SNIPPET_COLS} {SNIPPET_FROM}
        WHERE LOWER(s.abbreviation) LIKE ?1
           OR LOWER(s.title)        LIKE ?2
           OR LOWER(s.body)         LIKE ?2
        ORDER BY
            CASE WHEN LOWER(s.abbreviation) LIKE ?1 THEN 0 ELSE 1 END,
            s.abbreviation ASC
        LIMIT 10"#
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![prefix, contains], row_to_snippet)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn create(
    db: &DbHandle,
    abbreviation: &str,
    title: &str,
    body: &str,
    category_id: Option<i64>,
) -> Result<i64> {
    let now = Utc::now().timestamp_millis();
    let enc_body = crate::crypto::encrypt(body);
    let conn = db.lock();
    conn.execute(
        "INSERT INTO snippets (abbreviation, title, body, category_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![abbreviation.trim(), title.trim(), enc_body, category_id, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(
    db: &DbHandle,
    id: i64,
    abbreviation: &str,
    title: &str,
    body: &str,
    category_id: Option<i64>,
) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let enc_body = crate::crypto::encrypt(body);
    let conn = db.lock();
    conn.execute(
        "UPDATE snippets SET abbreviation = ?1, title = ?2, body = ?3, category_id = ?4, \
         updated_at = ?5 WHERE id = ?6",
        params![abbreviation.trim(), title.trim(), enc_body, category_id, now, id],
    )?;
    Ok(())
}

/// Assign (or clear, with `None`) a snippet's group.
pub fn set_category(db: &DbHandle, id: i64, category_id: Option<i64>) -> Result<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE snippets SET category_id = ?1 WHERE id = ?2",
        params![category_id, id],
    )?;
    Ok(())
}

// ── Category (group) CRUD ────────────────────────────────────────────────────

/// All groups with their snippet counts, ordered by `sort_order` then name.
pub fn list_categories(db: &DbHandle) -> Result<Vec<SnippetCategory>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.sort_order, \
                (SELECT COUNT(*) FROM snippets s WHERE s.category_id = c.id) \
         FROM snippet_categories c ORDER BY c.sort_order ASC, c.name ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SnippetCategory {
            id: r.get(0)?,
            name: r.get(1)?,
            sort_order: r.get(2)?,
            count: r.get(3)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// Create a group (or return the existing id if the name is taken). New groups
/// sort after the current last. Name is trimmed; empty is rejected.
pub fn create_category(db: &DbHandle, name: &str) -> Result<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("group name is empty"));
    }
    let conn = db.lock();
    if let Some(id) = conn
        .query_row("SELECT id FROM snippet_categories WHERE name = ?1", params![name], |r| r.get::<_, i64>(0))
        .optional()?
    {
        return Ok(id);
    }
    let next: i64 = conn
        .query_row("SELECT COALESCE(MAX(sort_order), 0) + 1 FROM snippet_categories", [], |r| r.get(0))
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO snippet_categories (name, sort_order) VALUES (?1, ?2)",
        params![name, next],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Rename a group. Fails if the new name collides with another group.
pub fn rename_category(db: &DbHandle, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("group name is empty"));
    }
    let conn = db.lock();
    conn.execute(
        "UPDATE snippet_categories SET name = ?1 WHERE id = ?2",
        params![name, id],
    )
    .map_err(|e| anyhow!("rename failed (name in use?): {e}"))?;
    Ok(())
}

/// Delete a group; its snippets become ungrouped (`category_id` → NULL).
pub fn delete_category(db: &DbHandle, id: i64) -> Result<()> {
    let conn = db.lock();
    conn.execute("UPDATE snippets SET category_id = NULL WHERE category_id = ?1", params![id])?;
    conn.execute("DELETE FROM snippet_categories WHERE id = ?1", params![id])?;
    Ok(())
}

/// Persist a new group order (ids in display order).
pub fn reorder_categories(db: &DbHandle, ids: &[i64]) -> Result<()> {
    let mut conn = db.lock();
    let tx = conn.transaction()?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE snippet_categories SET sort_order = ?1 WHERE id = ?2",
            params![i as i64, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Get a group's id by name, creating it if it doesn't exist. Used by the
/// seed + backup import to resolve a category NAME to a local id.
pub fn category_id_for_name(db: &DbHandle, name: &str) -> Result<Option<i64>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    create_category(db, name).map(Some)
}

pub fn delete(db: &DbHandle, id: i64) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
    Ok(())
}

/// Insert a snippet by abbreviation, or overwrite the existing row with the
/// same abbreviation. `created_at` is preserved on update. `category_id`:
/// `Some` sets/overwrites the group; `None` leaves an existing row's group
/// untouched (so a category-less re-import can't wipe the user's grouping).
pub fn upsert_by_abbreviation(
    db: &DbHandle,
    abbreviation: &str,
    title: &str,
    body: &str,
    category_id: Option<i64>,
) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let enc_body = crate::crypto::encrypt(body);
    let conn = db.lock();
    conn.execute(
        r#"
        INSERT INTO snippets (abbreviation, title, body, category_id, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(abbreviation) DO UPDATE SET
            title       = excluded.title,
            body        = excluded.body,
            category_id = CASE WHEN ?4 IS NOT NULL THEN ?4 ELSE snippets.category_id END,
            updated_at  = excluded.updated_at
        "#,
        params![abbreviation.trim(), title.trim(), enc_body, category_id, now],
    )?;
    Ok(())
}

/// Shape accepted in the JSON file. `title` is optional for convenience.
#[derive(Debug, Deserialize)]
struct ImportSnippet {
    abbreviation: String,
    #[serde(default)]
    title: String,
    body: String,
}

/// Allow either a top-level array, or `{ "snippets": [...] }`. Anything else
/// produces a parse error.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ImportPayload {
    Wrapped { snippets: Vec<ImportSnippet> },
    Bare(Vec<ImportSnippet>),
}

impl ImportPayload {
    fn into_snippets(self) -> Vec<ImportSnippet> {
        match self {
            ImportPayload::Wrapped { snippets } => snippets,
            ImportPayload::Bare(v) => v,
        }
    }
}

/// Parse a JSON document and upsert each snippet by abbreviation.
///
/// Accepts two top-level shapes:
/// 1. Bare array: `[{abbreviation, title?, body}, ...]`
/// 2. Wrapped:    `{"snippets": [...]}`
///
/// Per-row failures (empty fields, DB errors) are collected in the result
/// rather than aborting the whole import.
pub fn import_from_json(db: &DbHandle, json: &str) -> Result<ImportResult> {
    import_from_json_into(db, json, None)
}

/// As [`import_from_json`], but assigns every imported snippet to `category_id`
/// (used by the seed to drop the AI prompts / colours into their groups).
pub fn import_from_json_into(
    db: &DbHandle,
    json: &str,
    category_id: Option<i64>,
) -> Result<ImportResult> {
    let payload: ImportPayload = serde_json::from_str(json)
        .map_err(|e| anyhow!("invalid JSON: {e}"))?;
    let entries = payload.into_snippets();

    let mut result = ImportResult::default();
    for (idx, item) in entries.into_iter().enumerate() {
        let abbr = item.abbreviation.trim();
        let body = item.body.as_str();
        if abbr.is_empty() {
            result.skipped += 1;
            result
                .errors
                .push(format!("#{idx}: missing abbreviation"));
            continue;
        }
        if body.trim().is_empty() {
            result.skipped += 1;
            result
                .errors
                .push(format!("#{idx} ({abbr}): body is empty"));
            continue;
        }
        match upsert_by_abbreviation(db, abbr, &item.title, body, category_id) {
            Ok(()) => result.imported += 1,
            Err(e) => {
                result.skipped += 1;
                result.errors.push(format!("#{idx} ({abbr}): {e}"));
            }
        }
    }
    Ok(result)
}

fn row_to_snippet(row: &rusqlite::Row<'_>) -> rusqlite::Result<Snippet> {
    let raw_body: String = row.get(3)?;
    Ok(Snippet {
        id: row.get(0)?,
        abbreviation: row.get(1)?,
        title: row.get(2)?,
        body: crate::crypto::decrypt(&raw_body),
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        category: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let db = Arc::new(Mutex::new(conn));
        init_table(&db).unwrap();
        db
    }

    #[test]
    fn category_crud_and_grouping() {
        let db = test_db();
        let ai = create_category(&db, "AI Prompts").unwrap();
        // Duplicate name returns the same id (idempotent).
        assert_eq!(create_category(&db, "AI Prompts").unwrap(), ai);
        let colors = create_category(&db, "Colors").unwrap();

        let id = create(&db, "aiplan", "Plan", "body", Some(ai)).unwrap();
        create(&db, "reda700", "Red", "D50000", Some(colors)).unwrap();
        create(&db, "loose", "", "x", None).unwrap();

        // list_all joins the category NAME.
        let all = list_all(&db).unwrap();
        assert_eq!(all.iter().find(|s| s.abbreviation == "aiplan").unwrap().category.as_deref(), Some("AI Prompts"));
        assert_eq!(all.iter().find(|s| s.abbreviation == "loose").unwrap().category, None);

        // counts
        let cats = list_categories(&db).unwrap();
        assert_eq!(cats.iter().find(|c| c.name == "AI Prompts").unwrap().count, 1);
        assert_eq!(cats.iter().find(|c| c.name == "Colors").unwrap().count, 1);

        // move a snippet between groups
        set_category(&db, id, Some(colors)).unwrap();
        assert_eq!(list_categories(&db).unwrap().iter().find(|c| c.name == "Colors").unwrap().count, 2);

        // rename
        rename_category(&db, ai, "Prompts").unwrap();
        assert!(list_categories(&db).unwrap().iter().any(|c| c.name == "Prompts"));

        // delete → its snippets become ungrouped, not deleted
        delete_category(&db, colors).unwrap();
        assert_eq!(list_all(&db).unwrap().len(), 3, "snippets survive group deletion");
        assert!(list_all(&db).unwrap().iter().all(|s| s.category.as_deref() != Some("Colors")));
        assert!(!list_categories(&db).unwrap().iter().any(|c| c.name == "Colors"));
    }

    #[test]
    fn category_less_reimport_preserves_grouping() {
        let db = test_db();
        let g = create_category(&db, "G").unwrap();
        create(&db, "x", "t", "body1", Some(g)).unwrap();
        // Re-import the same abbreviation WITHOUT a category (plain JSON import)
        // must not wipe the existing group.
        import_from_json(&db, r#"[{"abbreviation":"x","body":"body2"}]"#).unwrap();
        let s = list_all(&db).unwrap();
        assert_eq!(s[0].category.as_deref(), Some("G"), "group preserved on category-less re-import");
        assert_eq!(s[0].body, "body2", "body still updated");
    }

    #[test]
    fn import_bare_array_inserts_each_row() {
        let db = test_db();
        let json = r#"
            [
                {"abbreviation": "mfg",  "title": "Greeting", "body": "Mit freundlichen Grüßen"},
                {"abbreviation": "addr", "body": "Some Street 1"}
            ]
        "#;
        let r = import_from_json(&db, json).unwrap();
        assert_eq!(r.imported, 2);
        assert_eq!(r.skipped, 0);
        assert!(r.errors.is_empty());
        assert_eq!(list_all(&db).unwrap().len(), 2);
    }

    #[test]
    fn import_wrapped_object_form_works() {
        let db = test_db();
        let json = r#"
            { "snippets": [{"abbreviation": "x", "body": "y"}] }
        "#;
        let r = import_from_json(&db, json).unwrap();
        assert_eq!(r.imported, 1);
        assert_eq!(list_all(&db).unwrap().len(), 1);
    }

    #[test]
    fn import_skips_rows_with_missing_fields() {
        let db = test_db();
        let json = r#"
            [
                {"abbreviation": "ok",   "body": "valid"},
                {"abbreviation": "",     "body": "no abbreviation"},
                {"abbreviation": "noBody","body": ""}
            ]
        "#;
        let r = import_from_json(&db, json).unwrap();
        assert_eq!(r.imported, 1);
        assert_eq!(r.skipped, 2);
        assert_eq!(r.errors.len(), 2);
        assert_eq!(list_all(&db).unwrap().len(), 1);
    }

    #[test]
    fn import_overwrites_existing_abbreviation() {
        let db = test_db();
        create(&db, "mfg", "Old", "Old body", None).unwrap();

        let json = r#"[{"abbreviation": "mfg", "title": "New", "body": "New body"}]"#;
        let r = import_from_json(&db, json).unwrap();
        assert_eq!(r.imported, 1);

        let rows = list_all(&db).unwrap();
        assert_eq!(rows.len(), 1, "no duplicate row created");
        assert_eq!(rows[0].title, "New");
        assert_eq!(rows[0].body, "New body");
    }

    #[test]
    fn import_invalid_json_returns_err() {
        let db = test_db();
        let r = import_from_json(&db, "not json");
        assert!(r.is_err());
    }

    #[test]
    fn import_trims_abbreviation_whitespace() {
        let db = test_db();
        let json = r#"[{"abbreviation": "  spaced  ", "body": "x"}]"#;
        import_from_json(&db, json).unwrap();
        let rows = list_all(&db).unwrap();
        assert_eq!(rows[0].abbreviation, "spaced");
    }

    #[test]
    fn find_by_exact_abbreviation_case_sensitive_first() {
        let db = test_db();
        create(&db, "MFG", "shouty", "uppercase body", None).unwrap();
        create(&db, "mfg", "lower", "lowercase body", None).unwrap();
        // Exact case wins over the case-insensitive fallback.
        let hit = find_by_exact_abbreviation(&db, "MFG").unwrap().unwrap();
        assert_eq!(hit.body, "uppercase body");
        let hit = find_by_exact_abbreviation(&db, "mfg").unwrap().unwrap();
        assert_eq!(hit.body, "lowercase body");
    }

    #[test]
    fn find_by_exact_abbreviation_falls_back_to_ci() {
        let db = test_db();
        create(&db, "mfg", "lower", "lowercase body", None).unwrap();
        // No "MFG" row exists — falls back to "mfg".
        let hit = find_by_exact_abbreviation(&db, "MFG").unwrap().unwrap();
        assert_eq!(hit.body, "lowercase body");
    }

    #[test]
    fn find_by_exact_abbreviation_returns_none_for_empty_input() {
        let db = test_db();
        create(&db, "mfg", "", "x", None).unwrap();
        assert!(find_by_exact_abbreviation(&db, "").unwrap().is_none());
        assert!(find_by_exact_abbreviation(&db, "   ").unwrap().is_none());
    }

    #[test]
    fn find_by_exact_abbreviation_trims_whitespace_before_lookup() {
        let db = test_db();
        create(&db, "mfg", "", "x", None).unwrap();
        let hit = find_by_exact_abbreviation(&db, "  mfg \n").unwrap().unwrap();
        assert_eq!(hit.abbreviation, "mfg");
    }

    #[test]
    fn find_by_exact_abbreviation_returns_none_for_unknown_abbr() {
        let db = test_db();
        create(&db, "mfg", "", "x", None).unwrap();
        assert!(find_by_exact_abbreviation(&db, "nope").unwrap().is_none());
    }

    #[test]
    fn get_by_id_returns_none_for_unknown_id() {
        let db = test_db();
        assert!(get_by_id(&db, 999_999).unwrap().is_none());
    }

    #[test]
    fn get_by_id_round_trips_a_created_snippet() {
        let db = test_db();
        let id = create(&db, "sig", "Signature", "Cheers,\nMartin", None).unwrap();
        let s = get_by_id(&db, id).unwrap().expect("just-created snippet must be retrievable");
        assert_eq!(s.abbreviation, "sig");
        assert_eq!(s.title, "Signature");
        assert_eq!(s.body, "Cheers,\nMartin");
    }

    #[test]
    fn update_changes_all_three_fields() {
        let db = test_db();
        let id = create(&db, "a", "old title", "old body", None).unwrap();
        update(&db, id, "b", "new title", "new body", None).unwrap();
        let s = get_by_id(&db, id).unwrap().unwrap();
        assert_eq!(s.abbreviation, "b");
        assert_eq!(s.title, "new title");
        assert_eq!(s.body, "new body");
    }

    #[test]
    fn delete_removes_only_the_targeted_snippet() {
        let db = test_db();
        let id1 = create(&db, "one", "", "1", None).unwrap();
        let id2 = create(&db, "two", "", "2", None).unwrap();
        delete(&db, id1).unwrap();
        assert!(get_by_id(&db, id1).unwrap().is_none());
        assert!(get_by_id(&db, id2).unwrap().is_some());
    }

    #[test]
    fn list_all_is_sorted_alphabetically_by_abbreviation() {
        let db = test_db();
        create(&db, "zeta", "", "z", None).unwrap();
        create(&db, "alpha", "", "a", None).unwrap();
        create(&db, "mu", "", "m", None).unwrap();
        let all = list_all(&db).unwrap();
        let abbrs: Vec<&str> = all.iter().map(|s| s.abbreviation.as_str()).collect();
        assert_eq!(abbrs, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn snippets_preserve_long_unicode_bodies() {
        let db = test_db();
        let body = "Hallo 🦀\n世界\n— éclair\n𝕳𝖊𝖑𝖑𝖔";
        let id = create(&db, "uni", "Unicode", body, None).unwrap();
        let s = get_by_id(&db, id).unwrap().unwrap();
        assert_eq!(s.body, body);
    }
}
