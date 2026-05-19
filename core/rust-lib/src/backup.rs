//! Full-app backup: serialize history + snippets + notes to a single JSON
//! document, and merge-restore from the same shape.
//!
//! Merge semantics on import:
//!   * snippets — upsert by `abbreviation` (existing rows are overwritten)
//!   * history  — upsert by SHA-256 hash (existing rows just bump
//!                `last_used_at`); duplicates are silently merged
//!   * notes    — appended verbatim with original timestamps; there is no
//!                natural dedup key, so re-importing the same backup will
//!                create duplicate notes (acceptable trade-off vs. data loss)

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::db::{self, DbHandle};
use crate::models::{ClipEntry, ContentType, NewClip};
use crate::notes::{self, Note};
use crate::snippets::{self, Snippet};

/// Bumped whenever the on-disk shape changes. Importing a newer-versioned
/// backup than this constant is rejected so users get a clear error rather
/// than silently losing fields.
pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Backup {
    pub version: u32,
    /// Unix-millis timestamp the backup was produced.
    pub exported_at: i64,
    #[serde(default)]
    pub history: Vec<ClipEntry>,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
    #[serde(default)]
    pub notes: Vec<Note>,
}

#[derive(Debug, Default, Serialize)]
pub struct BackupImportResult {
    pub history_imported: usize,
    pub snippets_imported: usize,
    pub notes_imported: usize,
    pub errors: Vec<String>,
}

/// Which sections of the database to include in an export. All true by
/// default — `ExportOptions::all()` matches the previous behaviour of
/// `export()`. Stored separately from [`Backup`] because the user picks
/// these in the Settings panel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ExportOptions {
    pub include_history: bool,
    pub include_snippets: bool,
    pub include_notes: bool,
}

impl ExportOptions {
    pub fn all() -> Self {
        Self {
            include_history: true,
            include_snippets: true,
            include_notes: true,
        }
    }
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self::all()
    }
}

/// Build the backup document from the live database. Empty `Vec`s are
/// written for sections the user opted out of via [`ExportOptions`].
pub fn export(db: &DbHandle, opts: ExportOptions) -> Result<Backup> {
    Ok(Backup {
        version: CURRENT_VERSION,
        exported_at: Utc::now().timestamp_millis(),
        // Pull *all* history entries (not the default 500 page size) when
        // included, so a backup is actually complete.
        history: if opts.include_history {
            db::list(db, 100_000, 0)?
        } else {
            Vec::new()
        },
        snippets: if opts.include_snippets {
            snippets::list_all(db)?
        } else {
            Vec::new()
        },
        notes: if opts.include_notes {
            notes::list_all(db)?
        } else {
            Vec::new()
        },
    })
}

/// Serialize the backup as pretty-printed JSON.
pub fn export_json(db: &DbHandle, opts: ExportOptions) -> Result<String> {
    let backup = export(db, opts)?;
    serde_json::to_string_pretty(&backup).map_err(Into::into)
}

/// Parse a backup JSON string and merge it into the live database.
pub fn import_json(db: &DbHandle, json: &str) -> Result<BackupImportResult> {
    let backup: Backup = serde_json::from_str(json)
        .map_err(|e| anyhow!("invalid backup JSON: {e}"))?;
    if backup.version > CURRENT_VERSION {
        return Err(anyhow!(
            "backup version {} is newer than this app supports ({CURRENT_VERSION})",
            backup.version
        ));
    }
    apply(db, backup)
}

fn apply(db: &DbHandle, backup: Backup) -> Result<BackupImportResult> {
    let mut result = BackupImportResult::default();

    // 1) Snippets — same upsert path used by snippet import, so behaviour
    //    is identical to JSON import.
    for (idx, s) in backup.snippets.iter().enumerate() {
        if s.abbreviation.trim().is_empty() {
            result
                .errors
                .push(format!("snippet #{idx}: empty abbreviation"));
            continue;
        }
        match snippets::upsert_by_abbreviation(db, &s.abbreviation, &s.title, &s.body) {
            Ok(()) => result.snippets_imported += 1,
            Err(e) => result
                .errors
                .push(format!("snippet #{idx} ({}): {e}", s.abbreviation)),
        }
    }

    // 2) History — re-use the existing dedup-by-hash upsert. Duplicates
    //    just bump `last_used_at`; new rows respect the 1 000-entry cap.
    for (idx, entry) in backup.history.iter().enumerate() {
        let new_clip = NewClip {
            content_type: entry.content_type,
            content_text: entry.content_text.clone(),
            content_data: entry.content_data.clone(),
            byte_size: entry.byte_size,
        };
        match db::upsert_clip(db, &new_clip) {
            Ok(_) => result.history_imported += 1,
            Err(e) => result.errors.push(format!("history #{idx}: {e}")),
        }
    }

    // 3) Notes — append verbatim. We deliberately skip dedup; re-importing
    //    the same file twice produces two copies of every note. Users who
    //    want a clean slate can use Clear All before importing.
    for (idx, note) in backup.notes.iter().enumerate() {
        // Coerce content_type from whatever the JSON decoded into; if it
        // was a string we don't know, fall back to text.
        let mut sanitized = note.clone();
        sanitized.content_type = match note.content_type {
            ContentType::Text
            | ContentType::Rtf
            | ContentType::Html
            | ContentType::Image
            | ContentType::Files => note.content_type,
        };
        match notes::append_imported(db, &sanitized) {
            Ok(_) => result.notes_imported += 1,
            Err(e) => result.errors.push(format!("note #{idx}: {e}")),
        }
    }

    Ok(result)
}

/// Helper used by tests and the IPC layer: drop everything and replace
/// with the contents of the backup. Intentionally NOT exposed via the UI
/// (yet) — too easy to lose data by accident.
#[allow(dead_code)]
pub fn replace_all(db: &DbHandle, backup: Backup) -> Result<BackupImportResult> {
    {
        let conn = db.lock();
        conn.execute("DELETE FROM entries", [])?;
        conn.execute("DELETE FROM snippets", [])?;
        conn.execute("DELETE FROM notes", [])?;
    }
    apply(db, backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ContentType, NewClip};
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn fresh_db() -> DbHandle {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE entries (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type  TEXT    NOT NULL,
                content_text  TEXT,
                content_data  BLOB,
                hash          TEXT    NOT NULL UNIQUE,
                byte_size     INTEGER NOT NULL,
                created_at    INTEGER NOT NULL,
                last_used_at  INTEGER NOT NULL
            );
            CREATE INDEX idx_last_used ON entries(last_used_at DESC);
            CREATE INDEX idx_hash ON entries(hash);
            "#,
        )
        .unwrap();
        let db = Arc::new(Mutex::new(conn));
        snippets::init_table(&db).unwrap();
        notes::init_table(&db).unwrap();
        db
    }

    fn seed(db: &DbHandle) {
        // 1 history entry, 1 snippet, 1 note.
        db::upsert_clip(
            db,
            &NewClip {
                content_type: ContentType::Text,
                content_text: "hello".into(),
                content_data: "hello".into(),
                byte_size: 5,
            },
        )
        .unwrap();
        snippets::create(db, "mfg", "Greeting", "Mit freundlichen Grüßen").unwrap();
        notes::create_text(db, "Pinned", "Important", "Inbox").unwrap();
    }

    #[test]
    fn export_and_import_roundtrip_into_empty_db() {
        let src = fresh_db();
        seed(&src);
        let json = export_json(&src, ExportOptions::all()).unwrap();

        let dst = fresh_db();
        let r = import_json(&dst, &json).unwrap();
        assert!(r.errors.is_empty(), "no errors: {:?}", r.errors);
        assert_eq!(r.history_imported, 1);
        assert_eq!(r.snippets_imported, 1);
        assert_eq!(r.notes_imported, 1);

        // Spot-check that data actually landed.
        assert_eq!(db::list(&dst, 10, 0).unwrap()[0].content_text, "hello");
        assert_eq!(snippets::list_all(&dst).unwrap()[0].abbreviation, "mfg");
        assert_eq!(notes::list_all(&dst).unwrap()[0].title, "Pinned");
    }

    #[test]
    fn import_into_populated_db_merges_via_dedup() {
        let db = fresh_db();
        seed(&db);

        // Re-import the same backup. History should dedupe; snippet should
        // upsert in place; notes will *append* (acceptable per design).
        let json = export_json(&db, ExportOptions::all()).unwrap();
        let r = import_json(&db, &json).unwrap();
        assert!(r.errors.is_empty(), "no errors: {:?}", r.errors);

        assert_eq!(
            db::list(&db, 10, 0).unwrap().len(),
            1,
            "history dedupes by hash"
        );
        assert_eq!(
            snippets::list_all(&db).unwrap().len(),
            1,
            "snippet upserts on abbreviation"
        );
        assert_eq!(
            notes::list_all(&db).unwrap().len(),
            2,
            "notes are appended (no natural dedup key)"
        );
    }

    #[test]
    fn export_with_only_snippets_emits_empty_other_sections() {
        let db = fresh_db();
        seed(&db);
        let opts = ExportOptions {
            include_history: false,
            include_snippets: true,
            include_notes: false,
        };
        let backup = export(&db, opts).unwrap();
        assert!(backup.history.is_empty(), "history should be empty");
        assert_eq!(backup.snippets.len(), 1, "snippet should be present");
        assert!(backup.notes.is_empty(), "notes should be empty");
    }

    #[test]
    fn export_with_all_off_emits_everything_empty() {
        let db = fresh_db();
        seed(&db);
        let opts = ExportOptions {
            include_history: false,
            include_snippets: false,
            include_notes: false,
        };
        let backup = export(&db, opts).unwrap();
        assert!(backup.history.is_empty());
        assert!(backup.snippets.is_empty());
        assert!(backup.notes.is_empty());
        // Version + timestamp must still be set so the file is parseable.
        assert_eq!(backup.version, CURRENT_VERSION);
        assert!(backup.exported_at > 0);
    }

    #[test]
    fn export_options_default_includes_everything() {
        let opts = ExportOptions::default();
        assert!(opts.include_history);
        assert!(opts.include_snippets);
        assert!(opts.include_notes);
    }

    #[test]
    fn import_rejects_newer_backup_version() {
        let db = fresh_db();
        let bad = format!(
            r#"{{"version": {}, "exported_at": 0, "history": [], "snippets": [], "notes": []}}"#,
            CURRENT_VERSION + 1
        );
        let err = import_json(&db, &bad).unwrap_err().to_string();
        assert!(err.contains("newer"), "got: {err}");
    }

    #[test]
    fn import_invalid_json_returns_err() {
        let db = fresh_db();
        assert!(import_json(&db, "not json").is_err());
    }

    #[test]
    fn replace_all_clears_then_inserts() {
        let db = fresh_db();
        seed(&db);
        // Build a tiny backup containing only one note, no snippet, no history.
        let backup = Backup {
            version: CURRENT_VERSION,
            exported_at: 0,
            history: vec![],
            snippets: vec![],
            notes: vec![Note {
                id: 0,
                content_type: ContentType::Text,
                content_text: "lonely".into(),
                content_data: "lonely".into(),
                title: "Lonely".into(),
                category: "".into(),
                byte_size: 6,
                created_at: 1,
                updated_at: 1,
            }],
        };
        let r = replace_all(&db, backup).unwrap();
        assert_eq!(r.notes_imported, 1);
        assert_eq!(snippets::list_all(&db).unwrap().len(), 0);
        assert_eq!(db::list(&db, 10, 0).unwrap().len(), 0);
        let notes = notes::list_all(&db).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "Lonely");
    }

    #[test]
    fn export_json_top_level_shape_is_versioned() {
        // Frontend parses the JSON shape — the top-level fields are part of
        // the contract documented in docs/backup.md.
        let db = fresh_db();
        seed(&db);
        let json = export_json(&db, ExportOptions::all()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("version").is_some());
        assert!(v.get("exported_at").is_some());
        assert!(v["history"].is_array());
        assert!(v["snippets"].is_array());
        assert!(v["notes"].is_array());
    }

    #[test]
    fn export_writes_current_version() {
        let db = fresh_db();
        let json = export_json(&db, ExportOptions::all()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["version"].as_u64().unwrap(), CURRENT_VERSION as u64);
    }

    #[test]
    fn export_options_all_includes_every_section() {
        let opts = ExportOptions::all();
        assert!(opts.include_history);
        assert!(opts.include_snippets);
        assert!(opts.include_notes);
    }

    #[test]
    fn export_options_default_includes_every_section() {
        // The TS frontend treats missing booleans as "yes please". Default
        // must match.
        let opts = ExportOptions::default();
        assert!(opts.include_history);
        assert!(opts.include_snippets);
        assert!(opts.include_notes);
    }

    #[test]
    fn import_handles_empty_sections_as_no_op() {
        let db = fresh_db();
        let blob = r#"{
            "version": 1,
            "exported_at": 0,
            "history": [],
            "snippets": [],
            "notes": []
        }"#;
        let r = import_json(&db, blob).unwrap();
        assert_eq!(r.history_imported, 0);
        assert_eq!(r.snippets_imported, 0);
        assert_eq!(r.notes_imported, 0);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn import_rejects_malformed_json_with_no_partial_writes() {
        let db = fresh_db();
        let before = (
            db::list(&db, 100, 0).unwrap().len(),
            snippets::list_all(&db).unwrap().len(),
            notes::list_all(&db).unwrap().len(),
        );
        assert!(import_json(&db, "{not json at all}").is_err());
        let after = (
            db::list(&db, 100, 0).unwrap().len(),
            snippets::list_all(&db).unwrap().len(),
            notes::list_all(&db).unwrap().len(),
        );
        assert_eq!(before, after, "malformed input must not leak partial writes");
    }
}
