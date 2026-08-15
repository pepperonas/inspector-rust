use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::DbHandle;

fn default_version() -> i64 {
    1
}

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
    /// Content-revision counter (v0.84.267, **protocol-compatible with cue**).
    /// Starts at 1; bumps by 1 only on a *content* change (abbreviation, title
    /// or body), never on an organisational change (group assign/reorder/rename/
    /// delete). Additive + `#[serde(default = 1)]` so backups without the field
    /// (older IR, older cue) load as v1.
    #[serde(default = "default_version")]
    pub version: i64,
}

/// cue-compatible version merge. `incoming` is the version in the imported
/// file (< 1 / missing → treated as 1); `local` is the current row's version
/// (`None` = the snippet doesn't exist locally yet). `content_differs` = the
/// incoming title/body differ from the local ones. Both sides converge to the
/// same number after a round-trip, and re-importing the same file is a no-op.
pub fn merge_version(incoming: i64, local: Option<i64>, content_differs: bool) -> i64 {
    let incoming = incoming.max(1);
    match local {
        None => incoming, // new snippet → max(incoming, 1)
        Some(local) if content_differs => incoming.max(local + 1),
        Some(local) => incoming.max(local),
    }
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

/// Snippet count + on-disk footprint (bytes of the stored columns). Shown in
/// Settings; there is no cap on the table.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct SnippetStorage {
    pub count: i64,
    pub bytes: i64,
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
            updated_at   INTEGER NOT NULL,
            version      INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_snippets_abbr ON snippets(abbreviation);
        CREATE TABLE IF NOT EXISTS snippet_categories (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT    NOT NULL UNIQUE,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS snippet_tombstones (
            abbreviation TEXT    PRIMARY KEY,
            version      INTEGER NOT NULL,
            deleted_at   INTEGER NOT NULL
        );
        "#,
    )?;
    // Lazy migrations for pre-existing snippet tables (idempotent — a second
    // run errors harmlessly because the column already exists). `DEFAULT 1`
    // back-fills every existing row to version 1.
    let _ = conn.execute("ALTER TABLE snippets ADD COLUMN category_id INTEGER", []);
    let _ = conn.execute(
        "ALTER TABLE snippets ADD COLUMN version INTEGER NOT NULL DEFAULT 1",
        [],
    );
    Ok(())
}

// ── SELECT with the joined category name ─────────────────────────────────────
const SNIPPET_COLS: &str =
    "s.id, s.abbreviation, s.title, s.body, s.created_at, s.updated_at, c.name, s.version";
const SNIPPET_FROM: &str =
    "FROM snippets s LEFT JOIN snippet_categories c ON c.id = s.category_id";

pub fn list_all(db: &DbHandle) -> Result<Vec<Snippet>> {
    let conn = db.lock();
    let sql = format!("SELECT {SNIPPET_COLS} {SNIPPET_FROM} ORDER BY s.abbreviation ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_snippet)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// How many snippets exist and how much on-disk space they occupy. Bytes are
/// the **actual stored** column bytes (`abbreviation + title + body`), so the
/// AES-encrypted body counts at its ciphertext size — that's the true storage
/// footprint. `CAST(... AS BLOB)` makes `LENGTH` count bytes, not characters,
/// so Unicode abbreviations/titles are measured correctly. There is no cap on
/// the snippets table — this is a footprint readout, not a limit.
pub fn storage_stats(db: &DbHandle) -> Result<SnippetStorage> {
    let conn = db.lock();
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), \
                COALESCE(SUM(LENGTH(CAST(abbreviation AS BLOB)) \
                           + LENGTH(CAST(title AS BLOB)) \
                           + LENGTH(CAST(body AS BLOB))), 0) \
         FROM snippets",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(SnippetStorage { count, bytes })
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

/// Match abbreviation prefix first, then title contains, then BODY contains —
/// up to 10 results.
///
/// The body pass runs in Rust over decrypted rows: the body column is
/// AES-encrypted at rest, so the historical `LOWER(s.body) LIKE` matched
/// CIPHERTEXT — body search silently never worked since encryption landed
/// (v0.47.0), and the expression was pure scan weight. (The unit tests never
/// caught it because crypto is a passthrough in the test process — SQL saw
/// plaintext there.) Decrypt-and-filter over the full set is ~1 ms at the
/// realistic table size (~300 seeded + user rows, short text bodies), and it
/// only runs when the plaintext passes left slots open.
pub fn find_by_query(db: &DbHandle, query: &str) -> Result<Vec<Snippet>> {
    const LIMIT: usize = 10;
    if query.is_empty() {
        return Ok(vec![]);
    }
    let q = query.to_lowercase();
    let prefix = format!("{}%", q);
    let contains = format!("%{}%", q);
    // Pass 1 — the plaintext columns (SQL, indexed-ish, historical ranking:
    // abbreviation prefix outranks title contains).
    let mut out: Vec<Snippet> = {
        let conn = db.lock();
        let sql = format!(
            r#"SELECT {SNIPPET_COLS} {SNIPPET_FROM}
            WHERE LOWER(s.abbreviation) LIKE ?1
               OR LOWER(s.title)        LIKE ?2
            ORDER BY
                CASE WHEN LOWER(s.abbreviation) LIKE ?1 THEN 0 ELSE 1 END,
                s.abbreviation ASC
            LIMIT 10"#
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![prefix, contains], row_to_snippet)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    // Pass 2 — body matches fill the remaining slots (weakest signal last).
    if out.len() < LIMIT {
        let seen: std::collections::HashSet<i64> = out.iter().map(|s| s.id).collect();
        for s in list_all(db)? {
            if out.len() >= LIMIT {
                break;
            }
            if !seen.contains(&s.id) && s.body.to_lowercase().contains(&q) {
                out.push(s);
            }
        }
    }
    Ok(out)
}


// ── Tombstones (deletion sync with cue) ──────────────────────────────────────
//
// A deleted (or renamed-away) abbreviation leaves a tombstone carrying the
// version the snippet had when it died. The cloud sync ([`crate::sync`]) uses
// them to propagate deletions instead of resurrecting the snippet; a
// recreation must start ABOVE the tombstone's version or the next sync cycle
// would delete it again. All helpers take `&Connection` — the callers already
// hold the DB lock (parking_lot Mutex is not reentrant).

pub(crate) fn record_tombstone_conn(
    conn: &Connection,
    abbreviation: &str,
    version: i64,
) -> rusqlite::Result<()> {
    let now = Utc::now().timestamp_millis();
    // deleted_at only refreshes when the version increases — re-received
    // tombstones from the peer must not keep the row alive forever.
    conn.execute(
        "INSERT INTO snippet_tombstones (abbreviation, version, deleted_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(abbreviation) DO UPDATE SET \
             deleted_at = CASE WHEN excluded.version > snippet_tombstones.version \
                               THEN excluded.deleted_at ELSE snippet_tombstones.deleted_at END, \
             version    = MAX(snippet_tombstones.version, excluded.version)",
        params![abbreviation, version, now],
    )?;
    Ok(())
}

pub(crate) fn tombstone_version_conn(
    conn: &Connection,
    abbreviation: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT version FROM snippet_tombstones WHERE abbreviation = ?1",
        params![abbreviation],
        |r| r.get(0),
    )
    .optional()
}

pub(crate) fn clear_tombstone_conn(conn: &Connection, abbreviation: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM snippet_tombstones WHERE abbreviation = ?1",
        params![abbreviation],
    )?;
    Ok(())
}

/// Version for a snippet (re)created over a tombstone: `max(version, ts + 1)`.
/// Clears the tombstone (the abbreviation is alive again).
pub(crate) fn resurrect_floor_conn(
    conn: &Connection,
    abbreviation: &str,
    version: i64,
) -> rusqlite::Result<i64> {
    match tombstone_version_conn(conn, abbreviation)? {
        Some(ts) => {
            clear_tombstone_conn(conn, abbreviation)?;
            Ok(version.max(ts + 1))
        }
        None => Ok(version),
    }
}

/// All tombstones as `(abbreviation, version, deleted_at_ms)` — the sync push.
pub fn list_tombstones(db: &DbHandle) -> Result<Vec<(String, i64, i64)>> {
    let conn = db.lock();
    let mut stmt =
        conn.prepare("SELECT abbreviation, version, deleted_at FROM snippet_tombstones")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

/// DbHandle wrappers for the sync engine.
pub fn record_tombstone(db: &DbHandle, abbreviation: &str, version: i64) -> Result<()> {
    let conn = db.lock();
    record_tombstone_conn(&conn, abbreviation, version).map_err(Into::into)
}

pub fn tombstone_version(db: &DbHandle, abbreviation: &str) -> Result<Option<i64>> {
    let conn = db.lock();
    tombstone_version_conn(&conn, abbreviation).map_err(Into::into)
}

pub fn clear_tombstone(db: &DbHandle, abbreviation: &str) -> Result<()> {
    let conn = db.lock();
    clear_tombstone_conn(&conn, abbreviation).map_err(Into::into)
}

/// Drop tombstones older than `max_age_ms` (sync housekeeping).
pub fn prune_tombstones(db: &DbHandle, max_age_ms: i64) -> Result<()> {
    let cutoff = Utc::now().timestamp_millis() - max_age_ms;
    let conn = db.lock();
    conn.execute(
        "DELETE FROM snippet_tombstones WHERE deleted_at < ?1",
        params![cutoff],
    )?;
    Ok(())
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
    // Recreating over a tombstone must land above it or sync re-deletes it.
    let version = resurrect_floor_conn(&conn, abbreviation.trim(), 1)?;
    conn.execute(
        "INSERT INTO snippets (abbreviation, title, body, category_id, created_at, updated_at, version) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
        params![abbreviation.trim(), title.trim(), enc_body, category_id, now, version],
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
    let (abbr, title) = (abbreviation.trim(), title.trim());
    let enc_body = crate::crypto::encrypt(body);
    let mut conn = db.lock();
    // ONE transaction for the whole edit (fixed 2026-08-16). The rename branch
    // below writes a tombstone for the OLD abbreviation *before* the UPDATE —
    // and that UPDATE can fail on the `abbreviation UNIQUE` constraint when the
    // user renames onto a name that already exists. Un-transactioned, the
    // tombstone survived the rejected edit: the cloud sync pushed it to cue,
    // cue echoed it back, and the still-live snippet was deleted on the next
    // pull. Either the whole edit lands or none of it does.
    let tx = conn.transaction()?;
    let conn = &tx;
    // Bump the version only on a *content* change (abbreviation / title / body).
    // A group-only edit — or a save that changed nothing — leaves it untouched.
    let current: Option<(String, String, String)> = conn
        .query_row(
            "SELECT abbreviation, title, body FROM snippets WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    crate::crypto::decrypt(&r.get::<_, String>(2)?),
                ))
            },
        )
        .optional()?;
    let content_changed = match &current {
        Some((a, t, b)) => a != abbr || t != title || b != body,
        None => true, // row vanished under us — treat as a write
    };
    // A rename is delete(old)+create(new) from the cloud sync's perspective:
    // tombstone the old key (at its pre-bump version) so the peer drops its
    // orphan copy, and land above any tombstone on the NEW key.
    if let Some((old_abbr, _, _)) = &current {
        if old_abbr != abbr {
            let old_version: i64 = conn
                .query_row(
                    "SELECT version FROM snippets WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .unwrap_or(1);
            record_tombstone_conn(conn, old_abbr, old_version)?;
        }
    }
    let bump = if content_changed { 1 } else { 0 };
    conn.execute(
        "UPDATE snippets SET abbreviation = ?1, title = ?2, body = ?3, category_id = ?4, \
         updated_at = ?5, version = version + ?7 WHERE id = ?6",
        params![abbr, title, enc_body, category_id, now, id, bump],
    )?;
    let bumped: i64 = conn
        .query_row("SELECT version FROM snippets WHERE id = ?1", params![id], |r| r.get(0))
        .unwrap_or(1);
    let floored = resurrect_floor_conn(conn, abbr, bumped)?;
    if floored != bumped {
        conn.execute(
            "UPDATE snippets SET version = ?1 WHERE id = ?2",
            params![floored, id],
        )?;
    }
    tx.commit()?;
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
    // Tombstone the abbreviation so the deletion propagates via cloud sync
    // instead of the peer resurrecting the snippet on the next cycle.
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT abbreviation, version FROM snippets WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((abbr, version)) = row {
        record_tombstone_conn(&conn, &abbr, version)?;
    }
    conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
    Ok(())
}

/// What an upsert does to the existing row's group. Three-valued on purpose:
/// a re-import that simply doesn't mention groups must not wipe the user's
/// grouping (`Keep`), but an editor round-trip has to be able to say
/// "this snippet is ungrouped now" (`Clear`) — which `Option<i64>` alone
/// can't express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryAssign {
    /// Leave an existing row's group untouched (new rows land ungrouped).
    Keep,
    /// Explicitly ungroup the snippet.
    Clear,
    /// Put the snippet in this group.
    Set(i64),
}

impl CategoryAssign {
    /// The `category_id` value written on INSERT (and on UPDATE when it applies).
    pub fn value(self) -> Option<i64> {
        match self {
            CategoryAssign::Set(id) => Some(id),
            CategoryAssign::Keep | CategoryAssign::Clear => None,
        }
    }

    /// Whether an UPDATE overwrites the existing group at all.
    pub fn applies(self) -> bool {
        !matches!(self, CategoryAssign::Keep)
    }
}

impl From<Option<i64>> for CategoryAssign {
    /// `Some(id)` → `Set`, `None` → `Keep` (the historical `Option` semantics).
    fn from(v: Option<i64>) -> Self {
        match v {
            Some(id) => CategoryAssign::Set(id),
            None => CategoryAssign::Keep,
        }
    }
}

/// Insert a snippet by abbreviation, or overwrite the existing row with the
/// same abbreviation. `created_at` is preserved on update. The group is
/// assigned per [`CategoryAssign`].
pub fn upsert_by_abbreviation(
    db: &DbHandle,
    abbreviation: &str,
    title: &str,
    body: &str,
    category: CategoryAssign,
    incoming_version: i64,
) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let (abbr, title) = (abbreviation.trim(), title.trim());
    let enc_body = crate::crypto::encrypt(body);
    let category_id = category.value();
    let applies = category.applies();
    let conn = db.lock();

    // Read the local row (if any) to run the cue-compatible version merge:
    // content differs → max(incoming, local+1); identical → max(incoming, local);
    // new → max(incoming, 1). Read the decrypted body so the comparison is over
    // plaintext (title is already stored trimmed + plaintext).
    let local: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT version, title, body FROM snippets WHERE abbreviation = ?1",
            params![abbr],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    crate::crypto::decrypt(&r.get::<_, String>(2)?),
                ))
            },
        )
        .optional()?;

    let content_differs = local
        .as_ref()
        .map(|(_, lt, lb)| lt != title || lb != body)
        .unwrap_or(true);
    let merged = merge_version(incoming_version, local.as_ref().map(|(v, _, _)| *v), content_differs);
    // An upsert brings the abbreviation (back) to life — land above any
    // tombstone so a re-import of a deleted snippet survives the next sync.
    let new_version = resurrect_floor_conn(&conn, abbr, merged)?;

    conn.execute(
        r#"
        INSERT INTO snippets (abbreviation, title, body, category_id, created_at, updated_at, version)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?7)
        ON CONFLICT(abbreviation) DO UPDATE SET
            title       = excluded.title,
            body        = excluded.body,
            category_id = CASE WHEN ?6 THEN ?4 ELSE snippets.category_id END,
            updated_at  = excluded.updated_at,
            version     = ?7
        "#,
        params![abbr, title, enc_body, category_id, now, applies, new_version],
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
        // The lean snippet shape carries no version → 1 (the merge treats it as
        // "content wins if changed" without inflating the number).
        match upsert_by_abbreviation(db, abbr, &item.title, body, category_id.into(), 1) {
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
        version: row.get(7)?,
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
    fn storage_stats_counts_rows_and_stored_bytes() {
        let db = test_db();
        // Empty table → zero, and the SUM must COALESCE to 0 (not NULL/err).
        let empty = storage_stats(&db).unwrap();
        assert_eq!(empty.count, 0);
        assert_eq!(empty.bytes, 0);

        // In the test process the cipher is uninitialised → body stored as
        // plaintext, so the byte count is exactly abbreviation+title+body.
        create(&db, "mfg", "Gruß", "Mit freundlichen Grüßen", None).unwrap();
        create(&db, "sig", "", "Cheers", None).unwrap();
        let s = storage_stats(&db).unwrap();
        assert_eq!(s.count, 2);
        let expected = "mfg".len()
            + "Gruß".len()
            + "Mit freundlichen Grüßen".len()
            + "sig".len()
            + "".len()
            + "Cheers".len();
        assert_eq!(s.bytes, expected as i64);
        // Umlauts are multi-byte → the CAST-AS-BLOB path counts bytes, not chars.
        assert!(s.bytes > ("mfg".chars().count()
            + "Gruß".chars().count()
            + "Mit freundlichen Grüßen".chars().count()
            + "sig".chars().count()
            + "Cheers".chars().count()) as i64);
    }

    #[test]
    fn storage_stats_has_no_thousand_row_cap() {
        // Guard the promise: the snippets table is never pruned, so well over
        // 1000 rows persist and are all counted.
        let db = test_db();
        for i in 0..1500 {
            create(&db, &format!("ab{i}"), "", "body", None).unwrap();
        }
        assert_eq!(storage_stats(&db).unwrap().count, 1500);
        assert_eq!(list_all(&db).unwrap().len(), 1500);
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
    fn find_by_query_matches_the_body() {
        // The restored feature (v0.105): body search is a Rust decrypt-filter,
        // because SQL LIKE only ever saw ciphertext in production.
        let db = test_db();
        create(&db, "sig", "Signatur", "Mit freundlichen Grüßen aus Berlin", None).unwrap();
        let hits = find_by_query(&db, "berlin").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].abbreviation, "sig");
    }

    #[test]
    fn find_by_query_ranks_prefix_over_title_over_body() {
        let db = test_db();
        // All three match "gruss" — via body, title, and abbreviation prefix.
        // Deliberately ordered so the OLD ranking (title+body tied, sorted by
        // abbreviation) would put the body row FIRST — the new ranking must
        // not: abbreviation prefix > title contains > body contains.
        create(&db, "aaa", "", "gruss im body", None).unwrap();
        create(&db, "zzz", "Der Gruss im Titel", "x", None).unwrap();
        create(&db, "grussformel", "", "x", None).unwrap();
        let hits = find_by_query(&db, "gruss").unwrap();
        let abbrevs: Vec<&str> = hits.iter().map(|s| s.abbreviation.as_str()).collect();
        assert_eq!(abbrevs, vec!["grussformel", "zzz", "aaa"]);
    }

    #[test]
    fn find_by_query_caps_at_ten_and_rejects_empty() {
        let db = test_db();
        for i in 0..12 {
            create(&db, &format!("ab{i:02}"), "", "das suchwort steckt im body", None).unwrap();
        }
        assert_eq!(find_by_query(&db, "suchwort").unwrap().len(), 10);
        assert!(find_by_query(&db, "").unwrap().is_empty());
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

    #[test]
    fn upsert_category_keep_clear_set() {
        let db = test_db();
        let g = create_category(&db, "AI Prompts").unwrap();
        let other = create_category(&db, "Colors").unwrap();
        create(&db, "aiplan", "Plan", "body", Some(g)).unwrap();

        // Keep: a category-less re-import must not wipe the grouping.
        upsert_by_abbreviation(&db, "aiplan", "Plan", "body2", CategoryAssign::Keep, 1).unwrap();
        let s = find_by_exact_abbreviation(&db, "aiplan").unwrap().unwrap();
        assert_eq!(s.category.as_deref(), Some("AI Prompts"));
        assert_eq!(s.body, "body2", "the rest of the row still updates");

        // Set: move to another group.
        upsert_by_abbreviation(&db, "aiplan", "Plan", "body2", CategoryAssign::Set(other), 1).unwrap();
        let s = find_by_exact_abbreviation(&db, "aiplan").unwrap().unwrap();
        assert_eq!(s.category.as_deref(), Some("Colors"));

        // Clear: explicitly ungroup.
        upsert_by_abbreviation(&db, "aiplan", "Plan", "body2", CategoryAssign::Clear, 1).unwrap();
        let s = find_by_exact_abbreviation(&db, "aiplan").unwrap().unwrap();
        assert_eq!(s.category, None);
    }

    #[test]
    fn upsert_inserts_new_row_ungrouped_for_keep_and_clear() {
        let db = test_db();
        upsert_by_abbreviation(&db, "fresh", "F", "b", CategoryAssign::Keep, 1).unwrap();
        assert_eq!(find_by_exact_abbreviation(&db, "fresh").unwrap().unwrap().category, None);
        upsert_by_abbreviation(&db, "fresh2", "F", "b", CategoryAssign::Clear, 1).unwrap();
        assert_eq!(find_by_exact_abbreviation(&db, "fresh2").unwrap().unwrap().category, None);
    }

    // ── Versioning (v0.84.267, cue-compatible) ───────────────────────────────

    fn version_of(db: &DbHandle, abbr: &str) -> i64 {
        find_by_exact_abbreviation(db, abbr).unwrap().unwrap().version
    }

    #[test]
    fn create_starts_at_v1() {
        let db = test_db();
        create(&db, "x", "T", "b", None).unwrap();
        assert_eq!(version_of(&db, "x"), 1);
    }

    #[test]
    fn update_bumps_only_on_content_change() {
        let db = test_db();
        let g = create_category(&db, "G").unwrap();
        let g2 = create_category(&db, "G2").unwrap();
        let id = create(&db, "aa", "Title", "body", None).unwrap();
        assert_eq!(version_of(&db, "aa"), 1);

        // Body change → v2.
        update(&db, id, "aa", "Title", "body v2", None).unwrap();
        assert_eq!(version_of(&db, "aa"), 2);
        // Title change → v3.
        update(&db, id, "aa", "Title v3", "body v2", None).unwrap();
        assert_eq!(version_of(&db, "aa"), 3);
        // Abbreviation change → v4.
        update(&db, id, "bb", "Title v3", "body v2", None).unwrap();
        assert_eq!(version_of(&db, "bb"), 4);

        // Group assignment → NO bump (still v4).
        update(&db, id, "bb", "Title v3", "body v2", Some(g)).unwrap();
        assert_eq!(version_of(&db, "bb"), 4);
        // Group change → NO bump.
        update(&db, id, "bb", "Title v3", "body v2", Some(g2)).unwrap();
        assert_eq!(version_of(&db, "bb"), 4);
        // A genuine no-op save → NO bump.
        update(&db, id, "bb", "Title v3", "body v2", Some(g2)).unwrap();
        assert_eq!(version_of(&db, "bb"), 4);
    }

    #[test]
    fn merge_version_matrix() {
        // New snippet → max(incoming, 1).
        assert_eq!(merge_version(0, None, false), 1);
        assert_eq!(merge_version(5, None, true), 5);
        // Content identical → max(incoming, local).
        assert_eq!(merge_version(1, Some(3), false), 3);
        assert_eq!(merge_version(9, Some(3), false), 9);
        // Content differs → max(incoming, local + 1).
        assert_eq!(merge_version(1, Some(3), true), 4);
        assert_eq!(merge_version(9, Some(3), true), 9);
        // Missing/0 incoming → treated as 1.
        assert_eq!(merge_version(0, Some(1), true), 2);
    }

    #[test]
    fn upsert_merge_respects_incoming_version() {
        let db = test_db();
        // New with an incoming version → adopts it.
        upsert_by_abbreviation(&db, "aa", "T", "b", CategoryAssign::Keep, 5).unwrap();
        assert_eq!(version_of(&db, "aa"), 5);
        // Re-import identical content, lower incoming → stays 5 (no-op).
        upsert_by_abbreviation(&db, "aa", "T", "b", CategoryAssign::Keep, 1).unwrap();
        assert_eq!(version_of(&db, "aa"), 5);
        // Changed content, lower incoming → local + 1.
        upsert_by_abbreviation(&db, "aa", "T", "b2", CategoryAssign::Keep, 1).unwrap();
        assert_eq!(version_of(&db, "aa"), 6);
        // Changed content, higher incoming → incoming wins.
        upsert_by_abbreviation(&db, "aa", "T", "b3", CategoryAssign::Keep, 20).unwrap();
        assert_eq!(version_of(&db, "aa"), 20);
    }

    #[test]
    fn migration_adds_version_column_and_backfills_v1() {
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;
        // A pre-versioning snippets table (no `version` column).
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"CREATE TABLE snippets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                abbreviation TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL DEFAULT '',
                body TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );"#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO snippets (abbreviation, title, body, created_at, updated_at) VALUES ('old','T','b',0,0)",
            [],
        )
        .unwrap();
        let db: DbHandle = Arc::new(Mutex::new(conn));
        init_table(&db).unwrap(); // runs the lazy ALTER
        assert_eq!(version_of(&db, "old"), 1, "existing rows back-fill to v1");
    }

    #[test]
    fn a_rejected_rename_leaves_no_tombstone_behind() {
        // REGRESSION (2026-08-16): `update` wrote the rename tombstone BEFORE
        // the UNIQUE-constrained UPDATE. When the user renamed onto a name that
        // already exists, the edit failed (correctly) but the tombstone stayed
        // — cloud sync pushed it, cue echoed it back, and the still-live
        // snippet was DELETED on the next pull. The whole edit is one
        // transaction now: it lands completely or not at all.
        let db = test_db();
        let keep = create(&db, "mfg", "Sign-off", "Mit freundlichen Grüßen", None).unwrap();
        create(&db, "sig", "Other", "body", None).unwrap();

        // Rename `mfg` onto the taken abbreviation `sig` → must fail.
        assert!(update(&db, keep, "sig", "Sign-off", "Mit freundlichen Grüßen", None).is_err());

        // The victim is untouched …
        let still = get_by_id(&db, keep).unwrap().unwrap();
        assert_eq!(still.abbreviation, "mfg");
        // … and — the load-bearing half — no tombstone was left for it.
        let tombstoned: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM snippet_tombstones WHERE abbreviation = 'mfg'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tombstoned, 0, "a rejected rename must not tombstone the live snippet");
    }

    #[test]
    fn a_successful_rename_still_tombstones_the_old_name() {
        // The counterpart: the transaction must not have disarmed the feature.
        let db = test_db();
        let id = create(&db, "old", "T", "body", None).unwrap();
        update(&db, id, "new", "T", "body", None).unwrap();
        let n: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM snippet_tombstones WHERE abbreviation = 'old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "the peer must learn the old key is gone");
    }
}
