//! Full-app backup: serialize history + snippets + notes + TOTP entries +
//! settings to a single JSON document, and merge-restore from the same shape.
//!
//! ## Encrypted backups (v2+)
//!
//! Backups can optionally be password-encrypted. The format is a JSON
//! envelope: `{ "encrypted": true, "kdf": "argon2id", "salt": "<b64>",
//! "nonce": "<b64>", "ciphertext": "<b64>" }`. The ciphertext is the
//! AES-256-GCM encryption of the plaintext backup JSON. On import, the
//! presence of `"encrypted": true` triggers a password prompt in the UI.
//!
//! ## Merge semantics on import:
//!   * snippets      — upsert by `abbreviation` (existing rows are overwritten)
//!   * history       — upsert by SHA-256 hash (existing rows just bump
//!                     `last_used_at`); duplicates are silently merged
//!   * notes         — appended verbatim with original timestamps; there is no
//!                     natural dedup key, so re-importing the same backup will
//!                     create duplicate notes (acceptable trade-off vs. data loss)
//!   * totp_entries  — upsert by (issuer, account); secret is re-encrypted with
//!                     the target machine's key on import
//!   * settings      — upsert by key (new values overwrite existing)

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::db::{self, DbHandle};
use crate::models::{ClipEntry, ContentType, NewClip};
use crate::notes::{self, Note};
use crate::snippets::{self, Snippet};
use crate::{crypto, settings, totp_store};

/// Bumped whenever the on-disk shape changes. Importing a newer-versioned
/// backup than this constant is rejected so users get a clear error rather
/// than silently losing fields.
pub const CURRENT_VERSION: u32 = 2;

/// A TOTP entry as stored in the backup. Contains the **plaintext**
/// base32 secret (decrypted at export time) so the backup is portable
/// across machines with different encryption keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpBackupEntry {
    pub issuer: String,
    pub account: String,
    /// Base32-encoded TOTP secret (plaintext in the backup file).
    pub secret: String,
    pub digits: u32,
    pub period: u32,
    pub algorithm: String,
    pub created_at: i64,
}

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
    /// TOTP entries with plaintext secrets (v2+).
    #[serde(default)]
    pub totp_entries: Vec<TotpBackupEntry>,
    /// App settings as key-value pairs (v2+).
    #[serde(default)]
    pub settings: HashMap<String, String>,
}

#[derive(Debug, Default, Serialize)]
pub struct BackupImportResult {
    pub history_imported: usize,
    pub snippets_imported: usize,
    pub notes_imported: usize,
    pub totp_imported: usize,
    pub settings_imported: usize,
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
    #[serde(default = "default_true")]
    pub include_totp: bool,
    #[serde(default = "default_true")]
    pub include_settings: bool,
}

fn default_true() -> bool {
    true
}

impl ExportOptions {
    pub fn all() -> Self {
        Self {
            include_history: true,
            include_snippets: true,
            include_notes: true,
            include_totp: true,
            include_settings: true,
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
        totp_entries: if opts.include_totp {
            export_totp_entries(db)?
        } else {
            Vec::new()
        },
        settings: if opts.include_settings {
            export_settings(db)?
        } else {
            HashMap::new()
        },
    })
}

/// Read all TOTP entries with their decrypted secrets for backup.
fn export_totp_entries(db: &DbHandle) -> Result<Vec<TotpBackupEntry>> {
    let entries = totp_store::list(db)?;
    let conn = db.lock();
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let secret_enc: String = conn
            .query_row(
                "SELECT secret_enc FROM totp_entries WHERE id = ?1",
                [e.id],
                |r| r.get(0),
            )
            .with_context(|| format!("read TOTP secret for id {}", e.id))?;
        let secret = crypto::decrypt(&secret_enc);
        out.push(TotpBackupEntry {
            issuer: e.issuer,
            account: e.account,
            secret,
            digits: e.digits,
            period: e.period,
            algorithm: e.algorithm,
            created_at: e.created_at,
        });
    }
    Ok(out)
}

/// Read all settings key-value pairs.
fn export_settings(db: &DbHandle) -> Result<HashMap<String, String>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows.into_iter().collect())
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
            Ok(id) => {
                // Restore pin + note (upsert_clip doesn't carry them).
                if entry.pinned {
                    let _ = db::set_pinned(db, id, true);
                }
                if let Some(n) = entry.note.as_deref().filter(|s| !s.is_empty()) {
                    let _ = db::set_note(db, id, Some(n));
                }
                result.history_imported += 1;
            }
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

    // 4) TOTP entries — upsert by (issuer, account). The plaintext secret
    //    from the backup is re-encrypted with this machine's key.
    for (idx, te) in backup.totp_entries.iter().enumerate() {
        if te.issuer.trim().is_empty() || te.secret.trim().is_empty() {
            result
                .errors
                .push(format!("totp #{idx}: empty issuer or secret"));
            continue;
        }
        match totp_store::add(
            db,
            &te.issuer,
            &te.account,
            &te.secret,
            te.digits,
            te.period,
            &te.algorithm,
        ) {
            Ok(_) => result.totp_imported += 1,
            Err(e) => {
                // If it already exists (duplicate), that's fine — count as imported.
                let msg = e.to_string();
                if msg.contains("UNIQUE constraint") || msg.contains("already exists") {
                    result.totp_imported += 1;
                } else {
                    result
                        .errors
                        .push(format!("totp #{idx} ({}): {e}", te.issuer));
                }
            }
        }
    }

    // 5) Settings — upsert by key.
    for (key, value) in &backup.settings {
        match settings::set(db, key, value) {
            Ok(()) => result.settings_imported += 1,
            Err(e) => result.errors.push(format!("setting '{key}': {e}")),
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

// ── Encrypted backup ────────────────────────────────────────────────

/// JSON envelope for an encrypted backup file.
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    /// Always `true` — presence of this field signals encrypted content.
    pub encrypted: bool,
    /// KDF identifier. Currently always "argon2id".
    pub kdf: String,
    /// Base64-encoded 16-byte salt for the KDF.
    pub salt: String,
    /// Base64-encoded 12-byte AES-GCM nonce.
    pub nonce: String,
    /// Base64-encoded AES-256-GCM ciphertext of the backup JSON.
    pub ciphertext: String,
}

/// Check if a JSON string is an encrypted backup (has `"encrypted": true`).
pub fn is_encrypted(json: &str) -> bool {
    // Quick heuristic check before full parse — avoids deserializing
    // a potentially large plaintext backup just to check a flag.
    if !json.contains("\"encrypted\"") {
        return false;
    }
    #[derive(Deserialize)]
    struct Probe {
        #[serde(default)]
        encrypted: bool,
    }
    serde_json::from_str::<Probe>(json)
        .map(|p| p.encrypted)
        .unwrap_or(false)
}

/// Encrypt a backup JSON string with a password using Argon2id + AES-256-GCM.
/// Returns the encrypted envelope as a pretty-printed JSON string.
pub fn encrypt_backup(plaintext_json: &str, password: &str) -> Result<String> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use argon2::Argon2;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use rand::RngCore;

    // Generate random salt (16 bytes) and nonce (12 bytes).
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // Derive 32-byte key from password via Argon2id.
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow!("Argon2id KDF failed: {e}"))?;

    // Encrypt the plaintext JSON with AES-256-GCM.
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow!("AES-256-GCM init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext_json.as_bytes())
        .map_err(|e| anyhow!("AES-256-GCM encrypt: {e}"))?;

    let envelope = EncryptedEnvelope {
        encrypted: true,
        kdf: "argon2id".into(),
        salt: B64.encode(salt),
        nonce: B64.encode(nonce_bytes),
        ciphertext: B64.encode(ciphertext),
    };

    serde_json::to_string_pretty(&envelope).map_err(Into::into)
}

/// Decrypt an encrypted backup envelope using the provided password.
/// Returns the decrypted backup JSON string on success.
pub fn decrypt_backup(envelope_json: &str, password: &str) -> Result<String> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use argon2::Argon2;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;

    let envelope: EncryptedEnvelope = serde_json::from_str(envelope_json)
        .context("invalid encrypted backup envelope")?;

    if envelope.kdf != "argon2id" {
        return Err(anyhow!("unsupported KDF: {} (expected argon2id)", envelope.kdf));
    }

    let salt = B64.decode(&envelope.salt).context("invalid salt base64")?;
    let nonce_bytes = B64.decode(&envelope.nonce).context("invalid nonce base64")?;
    let ciphertext = B64.decode(&envelope.ciphertext).context("invalid ciphertext base64")?;

    if salt.len() != 16 {
        return Err(anyhow!("salt must be 16 bytes, got {}", salt.len()));
    }
    if nonce_bytes.len() != 12 {
        return Err(anyhow!("nonce must be 12 bytes, got {}", nonce_bytes.len()));
    }

    // Derive key from password.
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow!("Argon2id KDF failed: {e}"))?;

    // Decrypt.
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow!("AES-256-GCM init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow!("decryption failed — wrong password or corrupted file"))?;

    String::from_utf8(plaintext).context("decrypted content is not valid UTF-8")
}

/// High-level: export + optionally encrypt in one call.
pub fn export_json_maybe_encrypted(
    db: &DbHandle,
    opts: ExportOptions,
    password: Option<&str>,
) -> Result<String> {
    let json = export_json(db, opts)?;
    match password {
        Some(pw) if !pw.is_empty() => encrypt_backup(&json, pw),
        _ => Ok(json),
    }
}

/// High-level: detect encrypted → decrypt if needed → import.
pub fn import_json_maybe_encrypted(
    db: &DbHandle,
    json: &str,
    password: Option<&str>,
) -> Result<BackupImportResult> {
    if is_encrypted(json) {
        let pw = password.ok_or_else(|| anyhow!("backup is encrypted — password required"))?;
        let decrypted = decrypt_backup(json, pw)?;
        import_json(db, &decrypted)
    } else {
        import_json(db, json)
    }
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
                last_used_at  INTEGER NOT NULL,
                pinned        INTEGER NOT NULL DEFAULT 0,
                note          TEXT
            );
            CREATE INDEX idx_last_used ON entries(last_used_at DESC);
            CREATE INDEX idx_hash ON entries(hash);
            "#,
        )
        .unwrap();
        let db = Arc::new(Mutex::new(conn));
        snippets::init_table(&db).unwrap();
        notes::init_table(&db).unwrap();
        settings::init_table(&db).unwrap();
        totp_store::init_table(&db).unwrap();
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
            include_totp: false,
            include_settings: false,
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
            include_totp: false,
            include_settings: false,
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
            totp_entries: vec![],
            settings: HashMap::new(),
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

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let db = fresh_db();
        seed(&db);
        let json = export_json(&db, ExportOptions::all()).unwrap();
        let encrypted = encrypt_backup(&json, "test-password-123").unwrap();

        // Must be detected as encrypted.
        assert!(is_encrypted(&encrypted));
        assert!(!is_encrypted(&json));

        // Decrypt with correct password.
        let decrypted = decrypt_backup(&encrypted, "test-password-123").unwrap();
        assert_eq!(decrypted, json);

        // Wrong password must fail.
        assert!(decrypt_backup(&encrypted, "wrong").is_err());
    }

    #[test]
    fn import_maybe_encrypted_plaintext() {
        let db = fresh_db();
        seed(&db);
        let json = export_json(&db, ExportOptions::all()).unwrap();

        let dst = fresh_db();
        let r = import_json_maybe_encrypted(&dst, &json, None).unwrap();
        assert!(r.errors.is_empty());
        assert_eq!(r.history_imported, 1);
    }

    #[test]
    fn import_maybe_encrypted_with_password() {
        let db = fresh_db();
        seed(&db);
        let json = export_json(&db, ExportOptions::all()).unwrap();
        let encrypted = encrypt_backup(&json, "secret").unwrap();

        let dst = fresh_db();
        // Without password: error.
        assert!(import_json_maybe_encrypted(&dst, &encrypted, None).is_err());
        // With wrong password: error.
        assert!(import_json_maybe_encrypted(&dst, &encrypted, Some("wrong")).is_err());
        // With correct password: success.
        let r = import_json_maybe_encrypted(&dst, &encrypted, Some("secret")).unwrap();
        assert!(r.errors.is_empty());
        assert_eq!(r.history_imported, 1);
    }

    #[test]
    fn v1_backup_imports_into_v2_without_totp_or_settings() {
        // A v1 backup has no totp_entries or settings fields — they
        // should default to empty via #[serde(default)].
        let db = fresh_db();
        let blob = r#"{
            "version": 1,
            "exported_at": 0,
            "history": [],
            "snippets": [],
            "notes": []
        }"#;
        let r = import_json(&db, blob).unwrap();
        assert_eq!(r.totp_imported, 0);
        assert_eq!(r.settings_imported, 0);
        assert!(r.errors.is_empty());
    }
}
