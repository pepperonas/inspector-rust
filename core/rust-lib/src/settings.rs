//! Tiny key/value settings store living in the same SQLite database as the
//! rest of the app. Used for things that need to survive a restart but
//! don't justify a dedicated table — e.g. the text-expander hotkey and
//! enabled flag.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::db::DbHandle;

pub fn init_table(db: &DbHandle) -> Result<()> {
    let conn = db.lock();
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;
    ensure_updated_at(&conn)?;
    Ok(())
}

/// Add the `updated_at` column (ms since epoch of the last VALUE change) to a
/// settings table created by an older build. Existing rows get 0 = "older than
/// anything", so a deliberate change on another Mac always wins over them.
/// Device sync uses the column to let the newest change of a key win.
pub(crate) fn ensure_updated_at(conn: &rusqlite::Connection) -> Result<()> {
    let has: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('settings') WHERE name = 'updated_at'")?
        .exists([])?;
    if !has {
        conn.execute(
            "ALTER TABLE settings ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Write `value` under `key` on an already-locked connection. The change time
/// only moves when the value really changes — a start-up routine that writes
/// the same value again must not look like a fresh decision to device sync.
pub(crate) fn upsert_locked(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<()> {
    upsert_at_locked(conn, key, value, now_ms())
}

/// Like [`upsert_locked`], but with an explicit change time (device sync
/// keeps the peer's time so the value doesn't bounce back as "newer").
pub(crate) fn upsert_at_locked(
    conn: &rusqlite::Connection,
    key: &str,
    value: &str,
    at: i64,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = CASE WHEN settings.value = excluded.value
                              THEN settings.updated_at
                              ELSE excluded.updated_at END
        "#,
        params![key, value, at],
    )?;
    Ok(())
}

/// Every setting with its change time, for device sync.
pub fn all_with_times(db: &DbHandle) -> Result<Vec<(String, String, i64)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT key, value, updated_at FROM settings")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Store a value with an explicit change time (device sync).
pub fn set_at(db: &DbHandle, key: &str, value: &str, at: i64) -> Result<()> {
    let conn = db.lock();
    upsert_at_locked(&conn, key, value, at)
}

pub fn get(db: &DbHandle, key: &str) -> Result<Option<String>> {
    let conn = db.lock();
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(v)
}

pub fn set(db: &DbHandle, key: &str, value: &str) -> Result<()> {
    let conn = db.lock();
    upsert_locked(&conn, key, value)
}

/// Remove a key (a no-op when it isn't there).
pub fn delete(db: &DbHandle, key: &str) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

/// Convenience: read a key, defaulting to `default` when missing.
pub fn get_or(db: &DbHandle, key: &str, default: &str) -> Result<String> {
    Ok(get(db, key)?.unwrap_or_else(|| default.to_string()))
}

/// Convenience: read a "true"/"false" flag, defaulting to `default` when
/// missing or unparsable.
pub fn get_bool(db: &DbHandle, key: &str, default: bool) -> Result<bool> {
    match get(db, key)? {
        Some(v) => match v.as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Ok(default),
        },
        None => Ok(default),
    }
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
        init_table(&db).unwrap();
        db
    }

    #[test]
    fn missing_key_returns_none() {
        let db = test_db();
        assert!(get(&db, "no.such.key").unwrap().is_none());
    }

    #[test]
    fn set_then_get_round_trip() {
        let db = test_db();
        set(&db, "foo", "bar").unwrap();
        assert_eq!(get(&db, "foo").unwrap().as_deref(), Some("bar"));
    }

    #[test]
    fn set_overwrites_existing_value() {
        let db = test_db();
        set(&db, "k", "v1").unwrap();
        set(&db, "k", "v2").unwrap();
        assert_eq!(get(&db, "k").unwrap().as_deref(), Some("v2"));
    }

    #[test]
    fn get_or_returns_default_for_missing_key() {
        let db = test_db();
        assert_eq!(get_or(&db, "k", "fallback").unwrap(), "fallback");
        set(&db, "k", "set").unwrap();
        assert_eq!(get_or(&db, "k", "fallback").unwrap(), "set");
    }

    #[test]
    fn get_bool_parses_known_truthy_and_falsy_strings() {
        let db = test_db();
        set(&db, "t1", "true").unwrap();
        set(&db, "t2", "1").unwrap();
        set(&db, "f1", "false").unwrap();
        set(&db, "f2", "0").unwrap();
        set(&db, "weird", "maybe").unwrap();
        assert!(get_bool(&db, "t1", false).unwrap());
        assert!(get_bool(&db, "t2", false).unwrap());
        assert!(!get_bool(&db, "f1", true).unwrap());
        assert!(!get_bool(&db, "f2", true).unwrap());
        // Unparsable → default (test both polarities).
        assert!(get_bool(&db, "weird", true).unwrap());
        assert!(!get_bool(&db, "weird", false).unwrap());
    }

    #[test]
    fn get_bool_returns_default_for_missing_key() {
        let db = test_db();
        assert!(get_bool(&db, "absent", true).unwrap());
        assert!(!get_bool(&db, "absent", false).unwrap());
    }

    #[test]
    fn set_overwrites_previous_value_for_same_key() {
        // Settings are key/value with the key as PRIMARY KEY — set should
        // upsert, not append/duplicate.
        let db = test_db();
        set(&db, "k", "first").unwrap();
        set(&db, "k", "second").unwrap();
        set(&db, "k", "third").unwrap();
        assert_eq!(get(&db, "k").unwrap(), Some("third".into()));
    }

    #[test]
    fn get_bool_accepts_common_truthy_strings() {
        let db = test_db();
        // Whatever the parser considers "true" — currently any non-"false"
        // stored value. Lock the contract.
        set(&db, "f1", "true").unwrap();
        assert!(get_bool(&db, "f1", false).unwrap());
        set(&db, "f2", "false").unwrap();
        assert!(!get_bool(&db, "f2", true).unwrap());
    }

    #[test]
    fn settings_persist_unicode_values() {
        // Settings hold things like the expander hotkey body preview / direct
        // slot JSON — must not corrupt non-ASCII content.
        let db = test_db();
        let v = "Hallo 🦀 世界 — éclair";
        set(&db, "u", v).unwrap();
        assert_eq!(get(&db, "u").unwrap().as_deref(), Some(v));
    }

    #[test]
    fn settings_persist_empty_string_explicitly() {
        // An explicit empty string is different from "key missing". Both
        // paths must work without conflating.
        let db = test_db();
        set(&db, "empty", "").unwrap();
        assert_eq!(get(&db, "empty").unwrap(), Some("".into()));
        assert_eq!(get(&db, "missing").unwrap(), None);
    }

    #[test]
    fn settings_handle_keys_with_dots_and_underscores() {
        // We use dotted keys ("expander.direct_slots", "paste.plain_text_only").
        // No clever parsing on the key — it's an opaque PK.
        let db = test_db();
        for k in &[
            "expander.direct_slots",
            "expander.hotkey",
            "paste.plain_text_only",
            "seed.default_snippets_v1",
        ] {
            set(&db, k, "X").unwrap();
            assert_eq!(get(&db, k).unwrap().as_deref(), Some("X"));
        }
    }
}
