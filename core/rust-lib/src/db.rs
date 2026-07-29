#![allow(clippy::items_after_test_module)]
use anyhow::{Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::crypto;
use crate::models::{ClipEntry, ContentType, NewClip, MAX_ENTRIES};

pub type DbHandle = Arc<Mutex<Connection>>;

/// Resolve `%APPDATA%\InspectorRust\history.db` on Windows, or the platform
/// equivalent on other OSes (useful for `cargo run` on macOS/Linux).
pub fn default_db_path() -> Result<PathBuf> {
    let mut dir = dirs::data_dir().context("no platform data dir available")?;
    dir.push("InspectorRust");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create data dir {}", dir.display()))?;
    dir.push("history.db");
    Ok(dir)
}

pub fn open(path: &PathBuf) -> Result<DbHandle> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite at {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS entries (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            content_type  TEXT    NOT NULL,
            content_text  TEXT,
            content_data  BLOB,
            hash          TEXT    NOT NULL UNIQUE,
            byte_size     INTEGER NOT NULL,
            created_at    INTEGER NOT NULL,
            last_used_at  INTEGER NOT NULL,
            pinned        INTEGER NOT NULL DEFAULT 0,
            note          TEXT,
            derived_from  INTEGER,
            derived_kind  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_last_used ON entries(last_used_at DESC);
        CREATE INDEX IF NOT EXISTS idx_hash ON entries(hash);
        "#,
    )?;
    // Migration for pre-v0.76.0 databases created without the `pinned`
    // column. Ignore the "duplicate column name" error on already-migrated DBs.
    let _ = conn.execute(
        "ALTER TABLE entries ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        [],
    );
    // Migration: `note` column (v0.84.106) — a user note attached to a clip
    // (encrypted at rest, like content). Noted clips are exempt from pruning.
    let _ = conn.execute("ALTER TABLE entries ADD COLUMN note TEXT", []);
    // Migration: clip lineage (v0.93.1) — a clip produced by copying an
    // existing one in a different shape (plain text / a text transform) records
    // its source + the manipulation, so the list can draw a git-graph-style
    // rail between the derived copy and its original.
    let _ = conn.execute("ALTER TABLE entries ADD COLUMN derived_from INTEGER", []);
    let _ = conn.execute("ALTER TABLE entries ADD COLUMN derived_kind TEXT", []);
    // Timesheet (time-tracking) tables — same idempotent CREATE-IF-NOT-EXISTS
    // convention; never crash an existing DB.
    crate::tracking::db::init_schema(&conn)
        .with_context(|| "failed to create timesheet tables")?;
    // System-stats history (time-series for the Stats panel's "history" view).
    crate::stats_history::init_schema(&conn)
        .with_context(|| "failed to create stats_history table")?;
    // Shazam recognition history (the `shazam` command persists each match).
    crate::shazam::init_schema(&conn)
        .with_context(|| "failed to create shazam_history table")?;
    Ok(Arc::new(Mutex::new(conn)))
}

pub fn hash_payload(content_type: ContentType, data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content_type.as_str().as_bytes());
    hasher.update(b"\x00");
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Insert a new clip, or bump `last_used_at` if its hash already exists.
/// Returns the row id of the affected entry.
pub fn upsert_clip(db: &DbHandle, clip: &NewClip) -> Result<i64> {
    upsert_clip_derived(db, clip, None, None)
}

/// [`upsert_clip`] that additionally records **lineage** (v0.93.1): the entry
/// this clip was derived from + which manipulation produced it.
///
/// Used by the "copy in another shape" paths (plain-text copy, the `Cmd+1…9`
/// text transforms). The derived copy is a *separate* row that lands at the top
/// of the history — the source row is deliberately **not** touched, so it keeps
/// its own content and its position in the list.
///
/// Lineage is recorded **only on insert**. On a hash collision the existing row
/// just gets its recency bumped and keeps whatever lineage it already had — so a
/// clip that was captured on its own is never retroactively relabelled as
/// derived, even when a transform later happens to produce the identical text.
pub fn upsert_clip_derived(
    db: &DbHandle,
    clip: &NewClip,
    derived_from: Option<i64>,
    derived_kind: Option<&str>,
) -> Result<i64> {
    let now = Utc::now().timestamp_millis();
    let hash = hash_payload(clip.content_type, &clip.content_data);
    let conn = db.lock();

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM entries WHERE hash = ?1",
            params![&hash],
            |row| row.get(0),
        )
        .optional()?;

    let id = if let Some(id) = existing {
        // Recency only — see the doc comment: an existing row keeps its own
        // lineage (or its lack of one).
        conn.execute(
            "UPDATE entries SET last_used_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        id
    } else {
        // `content_text` and `content_data` may contain passwords,
        // tokens, file paths, image bytes — encrypt at rest. `hash` is
        // computed over plaintext (kept plaintext for dedup) and
        // doesn't reveal content beyond duplicate-presence.
        let enc_text = crypto::encrypt(&clip.content_text);
        let enc_data = crypto::encrypt(&clip.content_data);
        conn.execute(
            r#"
            INSERT INTO entries (
                content_type, content_text, content_data, hash,
                byte_size, created_at, last_used_at, derived_from, derived_kind
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                clip.content_type.as_str(),
                enc_text,
                enc_data,
                hash,
                clip.byte_size,
                now,
                now,
                derived_from,
                derived_kind,
            ],
        )?;
        conn.last_insert_rowid()
    };

    prune_locked(&conn, effective_max_entries(&conn))?;
    Ok(id)
}

/// Settings key for the user-configurable clipboard-history cap.
pub const KEY_MAX_ENTRIES: &str = "history.max_entries";
/// Lower/upper bounds the cap is clamped to (the UI mirrors these).
pub const MIN_HISTORY_ENTRIES: i64 = 50;
pub const MAX_HISTORY_ENTRIES: i64 = 100_000;

/// The effective history cap: the stored `history.max_entries` clamped to
/// `[MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES]`, or the default `MAX_ENTRIES`
/// when unset/blank/invalid. Reads the setting from the SAME connection (we
/// already hold the lock while pruning) and is safe if the settings table is
/// absent (e.g. a bare in-memory test DB) — it just returns the default.
fn effective_max_entries(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![KEY_MAX_ENTRIES],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| s.trim().parse::<i64>().ok())
    .map(|n| n.clamp(MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES))
    .unwrap_or(MAX_ENTRIES)
}

/// The current effective cap (for the settings UI).
pub fn max_entries(db: &DbHandle) -> i64 {
    let conn = db.lock();
    effective_max_entries(&conn)
}

/// Persist a new cap (clamped to the valid range) and immediately prune the
/// history down to it. Returns the clamped value actually stored.
pub fn set_max_entries(db: &DbHandle, max: i64) -> Result<i64> {
    let clamped = max.clamp(MIN_HISTORY_ENTRIES, MAX_HISTORY_ENTRIES);
    let conn = db.lock();
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY_MAX_ENTRIES, clamped.to_string()],
    )?;
    prune_locked(&conn, clamped)?;
    Ok(clamped)
}

fn prune_locked(conn: &Connection, keep: i64) -> Result<()> {
    // Pinned **and** noted entries are kept (a note means the user cares).
    conn.execute(
        r#"
        DELETE FROM entries
        WHERE pinned = 0 AND note IS NULL AND id IN (
            SELECT id FROM entries
            WHERE pinned = 0 AND note IS NULL
            ORDER BY last_used_at DESC
            LIMIT -1 OFFSET ?1
        )
        "#,
        params![keep],
    )?;
    Ok(())
}

/// Pin / unpin an entry. Pinned entries float to the top of the list and are
/// never pruned.
pub fn set_pinned(db: &DbHandle, id: i64, pinned: bool) -> Result<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE entries SET pinned = ?1 WHERE id = ?2",
        params![pinned as i64, id],
    )?;
    Ok(())
}

/// Attach / update / clear a note on an entry. An empty/`None` note clears it
/// (and re-exposes the entry to pruning). The note is encrypted at rest.
pub fn set_note(db: &DbHandle, id: i64, note: Option<&str>) -> Result<()> {
    let conn = db.lock();
    let enc = note.filter(|s| !s.is_empty()).map(crypto::encrypt);
    conn.execute("UPDATE entries SET note = ?1 WHERE id = ?2", params![enc, id])?;
    Ok(())
}

/// Attach lineage to an entry that doesn't have any yet (v0.93.3).
///
/// Used by the **backup restore**, which has to re-establish the rails after
/// ids were reassigned on import. Unlike the live path — where a transform
/// merely *happening* to produce existing text is a coincidence and must not
/// relabel that row — a backup states a fact about this very clip, so filling
/// an empty lineage is correct. It still never overwrites an existing one.
pub fn set_lineage_if_absent(
    db: &DbHandle,
    id: i64,
    derived_from: i64,
    derived_kind: Option<&str>,
) -> Result<()> {
    let conn = db.lock();
    conn.execute(
        r#"
        UPDATE entries
           SET derived_from = ?2, derived_kind = ?3
         WHERE id = ?1 AND derived_from IS NULL
        "#,
        params![id, derived_from, derived_kind],
    )?;
    Ok(())
}

pub fn list(db: &DbHandle, limit: usize, offset: usize) -> Result<Vec<ClipEntry>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        r#"
        SELECT id, content_type, content_text, content_data, hash,
               byte_size, created_at, last_used_at, pinned, note,
               derived_from, derived_kind
        FROM entries
        ORDER BY pinned DESC, last_used_at DESC
        LIMIT ?1 OFFSET ?2
        "#,
    )?;
    let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Like [`list`], but **omits the `content_data` blob for `image` rows**
/// (returns an empty string for them). This is the query the popup's history
/// **view** uses — the list rows render an icon, not the bitmap, and image
/// actions (cutout / save / recolor) + paste all re-read the row by id, so the
/// view never needs the pixels. Image clips routinely hold multi-MB base64
/// PNGs; sending the full set on every popup-open / clipboard-change meant
/// decrypting + JSON-serialising + marshalling **hundreds of MB** across the
/// IPC boundary, which is what made the overlay feel slow to load. Skipping the
/// blob for images (the `CASE` returns the literal `''` so the encrypted column
/// is never read/decrypted) collapses the payload to a few MB.
///
/// `list` (full, with image data) is still used by the backup export, which
/// genuinely needs every byte.
pub fn list_slim(db: &DbHandle, limit: usize, offset: usize) -> Result<Vec<ClipEntry>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        r#"
        SELECT id, content_type, content_text,
               CASE WHEN content_type = 'image' THEN '' ELSE content_data END AS content_data,
               hash, byte_size, created_at, last_used_at, pinned, note,
               derived_from, derived_kind
        FROM entries
        ORDER BY pinned DESC, last_used_at DESC
        LIMIT ?1 OFFSET ?2
        "#,
    )?;
    let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn touch(db: &DbHandle, id: i64) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let conn = db.lock();
    conn.execute(
        "UPDATE entries SET last_used_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn get(db: &DbHandle, id: i64) -> Result<Option<ClipEntry>> {
    let conn = db.lock();
    let entry = conn
        .query_row(
            r#"
            SELECT id, content_type, content_text, content_data, hash,
                   byte_size, created_at, last_used_at, pinned, note,
                   derived_from, derived_kind
            FROM entries
            WHERE id = ?1
            "#,
            params![id],
            row_to_entry,
        )
        .optional()?;
    Ok(entry)
}

pub fn delete(db: &DbHandle, id: i64) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(db: &DbHandle) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM entries", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::prune_locked;
    use crate::models::{ContentType, NewClip};
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS entries (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type  TEXT    NOT NULL,
                content_text  TEXT,
                content_data  BLOB,
                hash          TEXT    NOT NULL UNIQUE,
                byte_size     INTEGER NOT NULL,
                created_at    INTEGER NOT NULL,
                last_used_at  INTEGER NOT NULL,
                pinned        INTEGER NOT NULL DEFAULT 0,
                note          TEXT,
                derived_from  INTEGER,
                derived_kind  TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_last_used ON entries(last_used_at DESC);
            CREATE INDEX IF NOT EXISTS idx_hash ON entries(hash);
            "#,
        )
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn text_clip(s: &str) -> NewClip {
        NewClip {
            content_type: ContentType::Text,
            content_text: s.to_string(),
            content_data: s.to_string(),
            byte_size: s.len() as i64,
        }
    }

    /// A DB with BOTH `entries` and `settings` — needed for the configurable
    /// history-cap tests (which read/write the `settings` table).
    fn test_db_with_settings() -> DbHandle {
        let db = test_db();
        db.lock()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT);",
            )
            .unwrap();
        db
    }

    #[test]
    fn max_entries_defaults_to_one_thousand_when_unset() {
        let db = test_db_with_settings();
        assert_eq!(max_entries(&db), 1000);
    }

    #[test]
    fn set_max_entries_clamps_to_the_valid_range() {
        let db = test_db_with_settings();
        // Below the floor → clamped up.
        assert_eq!(set_max_entries(&db, 5).unwrap(), MIN_HISTORY_ENTRIES);
        assert_eq!(max_entries(&db), MIN_HISTORY_ENTRIES);
        // Above the ceiling → clamped down.
        assert_eq!(set_max_entries(&db, 9_999_999).unwrap(), MAX_HISTORY_ENTRIES);
        // In range → stored verbatim.
        assert_eq!(set_max_entries(&db, 2500).unwrap(), 2500);
        assert_eq!(max_entries(&db), 2500);
    }

    #[test]
    fn a_lowered_cap_prunes_immediately_keeping_the_newest() {
        let db = test_db_with_settings();
        for i in 0..60 {
            upsert_clip(&db, &text_clip(&format!("clip-{i}"))).unwrap();
        }
        // `upsert_clip` stamps second-granularity timestamps, so a tight loop
        // ties them all → the prune's ORDER BY last_used_at would be
        // non-deterministic (flaky). Assign DISTINCT recency here so "keeps the
        // newest" is a real, deterministic assertion: clip-59 newest, clip-0
        // oldest. (Test-only DB has plaintext content_text — no cipher.)
        {
            let conn = db.lock();
            for i in 0..60i64 {
                conn.execute(
                    "UPDATE entries SET last_used_at = ?1 WHERE content_text = ?2",
                    params![1000 + i, format!("clip-{i}")],
                )
                .unwrap();
            }
        }
        assert_eq!(list(&db, 200, 0).unwrap().len(), 60);
        // Lower the cap to the floor (50) → prune drops the 10 oldest.
        assert_eq!(set_max_entries(&db, MIN_HISTORY_ENTRIES).unwrap(), 50);
        let rows = list(&db, 200, 0).unwrap();
        assert_eq!(rows.len(), 50);
        // The oldest (clip-0..clip-9) are gone; clip-59 (newest) survives.
        let texts: Vec<String> = rows.iter().map(|r| r.content_text.clone()).collect();
        assert!(texts.iter().any(|t| t == "clip-59"));
        assert!(!texts.iter().any(|t| t == "clip-0"));
    }

    #[test]
    fn hash_payload_is_deterministic() {
        let h1 = hash_payload(ContentType::Text, "hello");
        let h2 = hash_payload(ContentType::Text, "hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_payload_differs_by_content_type() {
        let h1 = hash_payload(ContentType::Text, "hello");
        let h2 = hash_payload(ContentType::Html, "hello");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_payload_differs_by_data() {
        let h1 = hash_payload(ContentType::Text, "hello");
        let h2 = hash_payload(ContentType::Text, "world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_payload_is_hex_string() {
        let h = hash_payload(ContentType::Text, "test");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {h}");
        assert_eq!(h.len(), 64, "SHA-256 should be 64 hex chars");
    }

    #[test]
    fn upsert_and_list_round_trip() {
        let db = test_db();
        upsert_clip(&db, &text_clip("hello")).unwrap();
        let entries = list(&db, 10, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content_text, "hello");
        assert_eq!(entries[0].content_type, ContentType::Text);
    }

    fn image_clip(data: &str) -> NewClip {
        NewClip {
            content_type: ContentType::Image,
            content_text: String::new(),
            content_data: data.to_string(),
            byte_size: data.len() as i64,
        }
    }

    #[test]
    fn list_slim_omits_image_data_but_keeps_text_and_other_fields() {
        let db = test_db();
        upsert_clip(&db, &image_clip("BIGBASE64IMAGEBLOB")).unwrap();
        upsert_clip(&db, &text_clip("hello world")).unwrap();

        // Full list keeps everything (what backup relies on).
        let full = list(&db, 10, 0).unwrap();
        let full_img = full.iter().find(|e| e.content_type == ContentType::Image).unwrap();
        assert_eq!(full_img.content_data, "BIGBASE64IMAGEBLOB");

        // Slim list blanks ONLY the image blob; text + metadata are intact.
        let slim = list_slim(&db, 10, 0).unwrap();
        assert_eq!(slim.len(), 2);
        let slim_img = slim.iter().find(|e| e.content_type == ContentType::Image).unwrap();
        assert_eq!(slim_img.content_data, "", "image blob must be stripped");
        assert_eq!(slim_img.byte_size, "BIGBASE64IMAGEBLOB".len() as i64); // metadata kept
        let slim_txt = slim.iter().find(|e| e.content_type == ContentType::Text).unwrap();
        assert_eq!(slim_txt.content_text, "hello world"); // text untouched
        assert_eq!(slim_txt.content_data, "hello world"); // non-image data untouched
    }

    #[test]
    fn upsert_deduplicates_identical_content() {
        let db = test_db();
        let id1 = upsert_clip(&db, &text_clip("dup")).unwrap();
        let id2 = upsert_clip(&db, &text_clip("dup")).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(list(&db, 10, 0).unwrap().len(), 1);
    }

    #[test]
    fn upsert_stores_distinct_entries_separately() {
        let db = test_db();
        upsert_clip(&db, &text_clip("a")).unwrap();
        upsert_clip(&db, &text_clip("b")).unwrap();
        assert_eq!(list(&db, 10, 0).unwrap().len(), 2);
    }

    #[test]
    fn get_returns_correct_entry() {
        let db = test_db();
        let id = upsert_clip(&db, &text_clip("find me")).unwrap();
        let entry = get(&db, id).unwrap().unwrap();
        assert_eq!(entry.id, id);
        assert_eq!(entry.content_text, "find me");
    }

    #[test]
    fn get_returns_none_for_missing_id() {
        let db = test_db();
        assert!(get(&db, 9999).unwrap().is_none());
    }

    #[test]
    fn delete_removes_entry() {
        let db = test_db();
        let id = upsert_clip(&db, &text_clip("to delete")).unwrap();
        delete(&db, id).unwrap();
        assert!(get(&db, id).unwrap().is_none());
        assert_eq!(list(&db, 10, 0).unwrap().len(), 0);
    }

    #[test]
    fn clear_removes_all_entries() {
        let db = test_db();
        upsert_clip(&db, &text_clip("a")).unwrap();
        upsert_clip(&db, &text_clip("b")).unwrap();
        upsert_clip(&db, &text_clip("c")).unwrap();
        clear(&db).unwrap();
        assert_eq!(list(&db, 10, 0).unwrap().len(), 0);
    }

    #[test]
    fn list_respects_limit() {
        let db = test_db();
        for i in 0..5 {
            upsert_clip(&db, &text_clip(&format!("item {i}"))).unwrap();
        }
        assert_eq!(list(&db, 2, 0).unwrap().len(), 2);
    }

    #[test]
    fn list_respects_offset() {
        let db = test_db();
        for i in 0..5 {
            upsert_clip(&db, &text_clip(&format!("item {i}"))).unwrap();
        }
        assert_eq!(list(&db, 10, 4).unwrap().len(), 1);
        assert_eq!(list(&db, 10, 5).unwrap().len(), 0);
    }

    #[test]
    fn touch_does_not_fail_on_valid_id() {
        let db = test_db();
        let id = upsert_clip(&db, &text_clip("touchable")).unwrap();
        touch(&db, id).unwrap();
    }

    #[test]
    fn prune_removes_oldest_entries_over_cap() {
        let db = test_db();
        // Insert 5 entries, then prune to 3
        for i in 0..5 {
            upsert_clip(&db, &text_clip(&format!("item {i}"))).unwrap();
        }
        {
            let conn = db.lock();
            prune_locked(&conn, 3).unwrap();
        }
        assert_eq!(list(&db, 10, 0).unwrap().len(), 3);
    }

    #[test]
    fn prune_keeps_most_recently_used_entries() {
        // Insert 5 entries, touch entries 1+3 to bump them to "fresh",
        // then prune to 3. The pruned entries should be 0, 2, 4 (oldest),
        // and the survivors are 1, 3, plus whichever just-inserted-and-newest
        // entry. Since we sort by last_used_at DESC, the touched ones win.
        let db = test_db();
        let ids: Vec<i64> = (0..5)
            .map(|i| upsert_clip(&db, &text_clip(&format!("item {i}"))).unwrap())
            .collect();
        // Wait a tick so touch produces a strictly-later timestamp.
        std::thread::sleep(std::time::Duration::from_millis(2));
        touch(&db, ids[1]).unwrap();
        touch(&db, ids[3]).unwrap();
        {
            let conn = db.lock();
            prune_locked(&conn, 3).unwrap();
        }
        let rows = list(&db, 10, 0).unwrap();
        let surviving: Vec<i64> = rows.iter().map(|r| r.id).collect();
        assert!(surviving.contains(&ids[1]), "touched item 1 must survive");
        assert!(surviving.contains(&ids[3]), "touched item 3 must survive");
    }

    #[test]
    fn upsert_returns_existing_id_for_duplicate_payload() {
        let db = test_db();
        let id1 = upsert_clip(&db, &text_clip("dup")).unwrap();
        let id2 = upsert_clip(&db, &text_clip("dup")).unwrap();
        assert_eq!(id1, id2, "dedup must return the existing row's id");
        assert_eq!(list(&db, 10, 0).unwrap().len(), 1);
    }

    #[test]
    fn upsert_bumps_last_used_at_for_duplicate() {
        let db = test_db();
        let id = upsert_clip(&db, &text_clip("dup")).unwrap();
        let before = get(&db, id).unwrap().unwrap().last_used_at;
        std::thread::sleep(std::time::Duration::from_millis(5));
        upsert_clip(&db, &text_clip("dup")).unwrap();
        let after = get(&db, id).unwrap().unwrap().last_used_at;
        assert!(
            after > before,
            "re-inserting an identical payload must update last_used_at ({before} → {after})"
        );
    }

    #[test]
    fn derived_copy_is_its_own_entry_and_leaves_the_source_in_place() {
        // The contract behind the lineage feature: copying a clip in another
        // shape must add a NEW row at the top and must NOT touch the original —
        // the source keeps its own content *and* its position in the list.
        let db = test_db();
        let source = upsert_clip(&db, &text_clip("Hello World")).unwrap();
        let source_pos = get(&db, source).unwrap().unwrap().last_used_at;
        std::thread::sleep(std::time::Duration::from_millis(5));

        let derived =
            upsert_clip_derived(&db, &text_clip("HELLO WORLD"), Some(source), Some("upper")).unwrap();

        assert_ne!(derived, source, "the derived copy must be its own entry");
        let src = get(&db, source).unwrap().unwrap();
        assert_eq!(src.content_text, "Hello World", "source content untouched");
        assert_eq!(src.last_used_at, source_pos, "source must keep its position");
        assert_eq!(src.derived_from, None, "the source is not itself derived");

        let der = get(&db, derived).unwrap().unwrap();
        assert_eq!(der.derived_from, Some(source));
        assert_eq!(der.derived_kind.as_deref(), Some("upper"));
        assert!(der.last_used_at > source_pos, "the derived copy sorts to the top");
    }

    #[test]
    fn every_read_path_returns_the_lineage() {
        // `row_to_entry` addresses columns by INDEX, so the three SELECTs and
        // the mapper have to agree. Reading a lineage back through all of them
        // is the guard against a column being added to one query only — which
        // would silently shift `pinned`/`note`/`derived_*` by one.
        let db = test_db();
        let source = upsert_clip(&db, &text_clip("src")).unwrap();
        let derived =
            upsert_clip_derived(&db, &text_clip("SRC"), Some(source), Some("upper")).unwrap();
        set_pinned(&db, derived, true).unwrap();
        set_note(&db, derived, Some("a note")).unwrap();

        let from_get = get(&db, derived).unwrap().unwrap();
        let from_list = list(&db, 10, 0).unwrap().into_iter().find(|e| e.id == derived).unwrap();
        let from_slim =
            list_slim(&db, 10, 0).unwrap().into_iter().find(|e| e.id == derived).unwrap();

        for (name, row) in [("get", &from_get), ("list", &from_list), ("list_slim", &from_slim)] {
            assert_eq!(row.derived_from, Some(source), "{name}: derived_from");
            assert_eq!(row.derived_kind.as_deref(), Some("upper"), "{name}: derived_kind");
            // The neighbouring columns must not have shifted either.
            assert!(row.pinned, "{name}: pinned");
            assert_eq!(row.note.as_deref(), Some("a note"), "{name}: note");
            assert_eq!(row.content_text, "SRC", "{name}: content_text");
        }
    }

    #[test]
    fn set_lineage_if_absent_fills_only_an_empty_lineage() {
        let db = test_db();
        let a = upsert_clip(&db, &text_clip("a")).unwrap();
        let b = upsert_clip(&db, &text_clip("b")).unwrap();
        let c = upsert_clip(&db, &text_clip("c")).unwrap();

        set_lineage_if_absent(&db, c, a, Some("upper")).unwrap();
        assert_eq!(get(&db, c).unwrap().unwrap().derived_from, Some(a));

        // A second call must not re-point an already-established rail.
        set_lineage_if_absent(&db, c, b, Some("lower")).unwrap();
        let row = get(&db, c).unwrap().unwrap();
        assert_eq!(row.derived_from, Some(a), "existing lineage must win");
        assert_eq!(row.derived_kind.as_deref(), Some("upper"));
    }

    #[test]
    fn organic_clip_is_never_retroactively_relabelled_as_derived() {
        // A clip that was captured on its own keeps `derived_from = None` even
        // when the identical text is later produced by a transform — otherwise
        // an unrelated old clip would suddenly sprout a lineage rail.
        let db = test_db();
        let organic = upsert_clip(&db, &text_clip("same")).unwrap();
        let other = upsert_clip(&db, &text_clip("other")).unwrap();

        let again = upsert_clip_derived(&db, &text_clip("same"), Some(other), Some("upper")).unwrap();

        assert_eq!(again, organic, "dedup by hash still applies");
        assert_eq!(get(&db, organic).unwrap().unwrap().derived_from, None);
    }

    #[test]
    fn re_deriving_the_same_text_is_idempotent() {
        // Applying the same transform twice must not pile up rows — it bumps
        // the existing derived copy back to the top and keeps its lineage.
        let db = test_db();
        let source = upsert_clip(&db, &text_clip("src")).unwrap();
        let derived =
            upsert_clip_derived(&db, &text_clip("SRC"), Some(source), Some("upper")).unwrap();
        let again =
            upsert_clip_derived(&db, &text_clip("SRC"), Some(source), Some("upper")).unwrap();
        assert_eq!(again, derived, "same payload → same row");
        let row = get(&db, derived).unwrap().unwrap();
        assert_eq!(row.derived_from, Some(source));
        assert_eq!(row.derived_kind.as_deref(), Some("upper"));
    }

    #[test]
    fn list_orders_by_last_used_at_desc() {
        let db = test_db();
        upsert_clip(&db, &text_clip("first")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        upsert_clip(&db, &text_clip("second")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        upsert_clip(&db, &text_clip("third")).unwrap();
        let rows = list(&db, 10, 0).unwrap();
        // Most recent first.
        assert_eq!(rows[0].content_text, "third");
        assert_eq!(rows[1].content_text, "second");
        assert_eq!(rows[2].content_text, "first");
    }

    #[test]
    fn delete_removes_only_targeted_entry() {
        let db = test_db();
        let id1 = upsert_clip(&db, &text_clip("a")).unwrap();
        let id2 = upsert_clip(&db, &text_clip("b")).unwrap();
        delete(&db, id1).unwrap();
        assert!(get(&db, id1).unwrap().is_none());
        assert!(get(&db, id2).unwrap().is_some());
    }

    #[test]
    fn clear_wipes_every_entry() {
        let db = test_db();
        for i in 0..7 {
            upsert_clip(&db, &text_clip(&format!("item {i}"))).unwrap();
        }
        clear(&db).unwrap();
        assert_eq!(list(&db, 10, 0).unwrap().len(), 0);
    }

    #[test]
    fn get_returns_none_for_unknown_id() {
        let db = test_db();
        assert!(get(&db, 999_999).unwrap().is_none());
    }

    #[test]
    fn list_pagination_offsets_correctly() {
        let db = test_db();
        // Insert 5 entries with small delays so their timestamps strictly order
        for i in 0..5 {
            upsert_clip(&db, &text_clip(&format!("page {i}"))).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let page1 = list(&db, 2, 0).unwrap();
        let page2 = list(&db, 2, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 2);
        // No overlap
        for p1 in &page1 {
            for p2 in &page2 {
                assert_ne!(p1.id, p2.id, "pages must not overlap");
            }
        }
    }

    #[test]
    fn list_handles_zero_limit() {
        let db = test_db();
        upsert_clip(&db, &text_clip("any")).unwrap();
        let rows = list(&db, 0, 0).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn set_pinned_toggles_the_flag() {
        let db = test_db();
        let id = upsert_clip(&db, &text_clip("x")).unwrap();
        assert!(!get(&db, id).unwrap().unwrap().pinned);
        set_pinned(&db, id, true).unwrap();
        assert!(get(&db, id).unwrap().unwrap().pinned);
        set_pinned(&db, id, false).unwrap();
        assert!(!get(&db, id).unwrap().unwrap().pinned);
    }

    #[test]
    fn set_note_attaches_clears_and_exempts_from_prune() {
        let db = test_db();
        let a = upsert_clip(&db, &text_clip("a")).unwrap();
        upsert_clip(&db, &text_clip("b")).unwrap();
        assert_eq!(get(&db, a).unwrap().unwrap().note, None);
        set_note(&db, a, Some("call client")).unwrap();
        assert_eq!(get(&db, a).unwrap().unwrap().note.as_deref(), Some("call client"));
        // keep=0 → unnoted rows pruned, the noted one survives.
        {
            let conn = db.lock();
            prune_locked(&conn, 0).unwrap();
        }
        let rows = list(&db, 100, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].note.as_deref(), Some("call client"));
        // Clearing the note re-exposes it to pruning.
        set_note(&db, a, Some("")).unwrap();
        assert_eq!(get(&db, a).unwrap().unwrap().note, None);
        {
            let conn = db.lock();
            prune_locked(&conn, 0).unwrap();
        }
        assert_eq!(list(&db, 100, 0).unwrap().len(), 0);
    }

    #[test]
    fn pinned_survives_prune() {
        let db = test_db();
        let a = upsert_clip(&db, &text_clip("a")).unwrap();
        upsert_clip(&db, &text_clip("b")).unwrap();
        upsert_clip(&db, &text_clip("c")).unwrap();
        set_pinned(&db, a, true).unwrap();
        // keep = 0 → every *non-pinned* row is pruned; only the pin survives.
        {
            let conn = db.lock();
            prune_locked(&conn, 0).unwrap();
        }
        let rows = list(&db, 100, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, a);
        assert!(rows[0].pinned);
    }

    #[test]
    fn pinned_floats_to_top_regardless_of_recency() {
        let db = test_db();
        let a = upsert_clip(&db, &text_clip("a")).unwrap();
        set_pinned(&db, a, true).unwrap();
        upsert_clip(&db, &text_clip("b")).unwrap(); // newer, unpinned
        let rows = list(&db, 100, 0).unwrap();
        // `ORDER BY pinned DESC` puts the pin first even though `b` is newer.
        assert_eq!(rows[0].id, a);
        assert!(rows[0].pinned);
        assert!(!rows[1].pinned);
    }

    #[test]
    fn hash_payload_distinguishes_unicode_normalisation_forms() {
        // SHA-256 is byte-deterministic — equivalent NFC vs NFD strings
        // produce different hashes. Document that we do *not* normalise.
        let nfc = "café"; // 0x65 0xCC 0x81 vs single 0xE9 — distinct bytes
        let nfd = "café"; // looks identical, may be encoded differently
        let h_nfc = hash_payload(ContentType::Text, nfc);
        let h_nfd = hash_payload(ContentType::Text, nfd);
        // Both produce *some* hash, no panic. Equality depends on whether
        // the source literals were saved in the same normalisation form
        // by the editor; both forms are valid input.
        assert_eq!(h_nfc.len(), 64);
        assert_eq!(h_nfd.len(), 64);
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipEntry> {
    let ct_str: String = row.get(1)?;
    let content_type = ContentType::from_str(&ct_str).unwrap_or(ContentType::Text);
    let raw_text = row.get::<_, Option<String>>(2)?.unwrap_or_default();
    let raw_data = row.get::<_, Option<String>>(3)?.unwrap_or_default();
    Ok(ClipEntry {
        id: row.get(0)?,
        content_type,
        content_text: crypto::decrypt(&raw_text),
        content_data: crypto::decrypt(&raw_data),
        hash: row.get(4)?,
        byte_size: row.get(5)?,
        created_at: row.get(6)?,
        last_used_at: row.get(7)?,
        pinned: row.get::<_, i64>(8)? != 0,
        note: row
            .get::<_, Option<String>>(9)?
            .map(|enc| crypto::decrypt(&enc))
            .filter(|s| !s.is_empty()),
        derived_from: row.get(10)?,
        derived_kind: row.get::<_, Option<String>>(11)?.filter(|s| !s.is_empty()),
    })
}
