#![allow(clippy::doc_overindented_list_items)]
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
//!   * timesheet     — (v3, opt-in) sessions dedup by `started_at`, events by
//!                     (session, started_at, app, source), claude-turns by
//!                     (event, ts); ids are remapped, encrypted titles/urls are
//!                     re-encrypted with the target machine's key

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
/// than silently losing fields. A file only *claims* version 3 when it
/// actually carries the v3 `timesheet` section — timesheet-less exports keep
/// writing version 2 so older app builds can still import them.
pub const CURRENT_VERSION: u32 = 3;

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

// ── Timesheet backup (v3, opt-in) ────────────────────────────────────────────
//
// Portable snapshots of the `track_*` tables. `window_title` / `url` are
// stored **plaintext** in the backup (decrypted at export) — like TOTP
// secrets — and re-encrypted with the target machine's key on import.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSessionBackup {
    pub id: i64,
    pub label: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackEventBackup {
    pub id: i64,
    pub session_id: i64,
    pub app_name: String,
    pub app_id: Option<String>,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub host: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
    pub source: String,
    pub is_idle: bool,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_s: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackClaudeTurnBackup {
    pub event_id: i64,
    pub ts: i64,
    pub model: Option<String>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackCategoryBackup {
    pub app_name: String,
    pub category: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimesheetBackup {
    #[serde(default)]
    pub sessions: Vec<TrackSessionBackup>,
    #[serde(default)]
    pub events: Vec<TrackEventBackup>,
    #[serde(default)]
    pub claude_turns: Vec<TrackClaudeTurnBackup>,
    #[serde(default)]
    pub categories: Vec<TrackCategoryBackup>,
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
    /// Snippet groups (names + order) so empty groups + ordering survive; each
    /// snippet references its group by NAME via `Snippet.category`. Additive
    /// with a serde default → older backups (no groups) still import.
    #[serde(default)]
    pub snippet_categories: Vec<SnippetCategoryBackup>,
    #[serde(default)]
    pub notes: Vec<Note>,
    /// TOTP entries with plaintext secrets (v2+).
    #[serde(default)]
    pub totp_entries: Vec<TotpBackupEntry>,
    /// App settings as key-value pairs (v2+).
    #[serde(default)]
    pub settings: HashMap<String, String>,
    /// Timesheet tracking data (v3+, opt-in — absent unless the user ticked
    /// "include timesheet" at export time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timesheet: Option<TimesheetBackup>,
}

/// A snippet group in the backup — name + order (ids are machine-local).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetCategoryBackup {
    pub name: String,
    #[serde(default)]
    pub sort_order: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct BackupImportResult {
    pub history_imported: usize,
    pub snippets_imported: usize,
    pub notes_imported: usize,
    pub totp_imported: usize,
    pub settings_imported: usize,
    /// Timesheet events imported (v3 backups only; 0 otherwise).
    pub timesheet_imported: usize,
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
    /// Timesheet tracking data — **opt-in** (defaults false everywhere,
    /// including `all()`): it's often large, machine-bound telemetry, and
    /// including it bumps the file's format version to 3.
    #[serde(default)]
    pub include_timesheet: bool,
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
            include_timesheet: false,
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
        // Claim v3 only when the file actually carries the v3 section, so
        // timesheet-less backups stay importable by older app builds.
        version: if opts.include_timesheet { 3 } else { 2 },
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
        snippet_categories: if opts.include_snippets {
            snippets::list_categories(db)?
                .into_iter()
                .map(|c| SnippetCategoryBackup { name: c.name, sort_order: c.sort_order })
                .collect()
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
        timesheet: if opts.include_timesheet {
            Some(export_timesheet(db)?)
        } else {
            None
        },
    })
}

/// Read the whole timesheet (`track_*` tables) with titles/urls decrypted.
fn export_timesheet(db: &DbHandle) -> Result<TimesheetBackup> {
    let conn = db.lock();
    let sessions = conn
        .prepare("SELECT id, label, started_at, ended_at, status FROM track_sessions ORDER BY id")?
        .query_map([], |r| {
            Ok(TrackSessionBackup {
                id: r.get(0)?,
                label: r.get(1)?,
                started_at: r.get(2)?,
                ended_at: r.get(3)?,
                status: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    let events = conn
        .prepare(
            "SELECT id, session_id, app_name, app_id, window_title, url, host, category, \
             project, source, is_idle, started_at, ended_at, duration_s \
             FROM track_events ORDER BY id",
        )?
        .query_map([], |r| {
            Ok(TrackEventBackup {
                id: r.get(0)?,
                session_id: r.get(1)?,
                app_name: r.get(2)?,
                app_id: r.get(3)?,
                window_title: r.get::<_, Option<String>>(4)?.map(|v| crypto::decrypt(&v)),
                url: r.get::<_, Option<String>>(5)?.map(|v| crypto::decrypt(&v)),
                host: r.get(6)?,
                category: r.get(7)?,
                project: r.get(8)?,
                source: r.get(9)?,
                is_idle: r.get::<_, i64>(10)? != 0,
                started_at: r.get(11)?,
                ended_at: r.get(12)?,
                duration_s: r.get(13)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    let claude_turns = conn
        .prepare("SELECT event_id, ts, model, tokens_in, tokens_out FROM track_claude_turns ORDER BY id")?
        .query_map([], |r| {
            Ok(TrackClaudeTurnBackup {
                event_id: r.get(0)?,
                ts: r.get(1)?,
                model: r.get(2)?,
                tokens_in: r.get(3)?,
                tokens_out: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    let categories = conn
        .prepare("SELECT app_name, category FROM track_categories ORDER BY app_name")?
        .query_map([], |r| {
            Ok(TrackCategoryBackup { app_name: r.get(0)?, category: r.get(1)? })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(TimesheetBackup { sessions, events, claude_turns, categories })
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

/// Apply just the snippet sections (groups first, then snippets) of a backup.
/// Shared by the full import and the snippets-only import so both resolve
/// groups identically. Returns `(imported, errors)`.
fn apply_snippets(db: &DbHandle, backup: &Backup) -> (usize, Vec<String>) {
    let mut imported = 0usize;
    let mut errors = Vec::new();

    // Groups first — create every one by name (preserves EMPTY groups + order);
    // each snippet then resolves its group by name.
    for cat in &backup.snippet_categories {
        let _ = snippets::create_category(db, &cat.name);
    }

    // Snippets — same upsert path used by the lean snippet import, so behaviour
    // is identical. Each snippet's group (by NAME) is resolved to a local id
    // (created if missing).
    for (idx, s) in backup.snippets.iter().enumerate() {
        if s.abbreviation.trim().is_empty() {
            errors.push(format!("snippet #{idx}: empty abbreviation"));
            continue;
        }
        let assign = match &s.category {
            // A named group: resolve to a local id (created if missing).
            Some(name) if !name.trim().is_empty() => {
                match snippets::category_id_for_name(db, name) {
                    Ok(Some(id)) => snippets::CategoryAssign::Set(id),
                    // Couldn't resolve → don't touch the existing grouping.
                    _ => snippets::CategoryAssign::Keep,
                }
            }
            // Empty string = the explicit "ungroup me" sentinel (an external
            // editor round-trip needs a way to say this; `null`/absent can't,
            // because that has to stay "leave the grouping alone").
            Some(_) => snippets::CategoryAssign::Clear,
            None => snippets::CategoryAssign::Keep,
        };
        match snippets::upsert_by_abbreviation(
            db,
            &s.abbreviation,
            &s.title,
            &s.body,
            assign,
            s.version,
        ) {
            Ok(()) => imported += 1,
            Err(e) => errors.push(format!("snippet #{idx} ({}): {e}", s.abbreviation)),
        }
    }
    (imported, errors)
}

/// Export **only** snippets + their groups as a backup document. The other
/// sections are written empty, which the importer treats as "don't touch" —
/// so the file is a safe, self-contained snippet exchange format (this is what
/// an external snippet editor round-trips through).
pub fn export_snippets_json(db: &DbHandle) -> Result<String> {
    let opts = ExportOptions {
        include_history: false,
        include_snippets: true,
        include_notes: false,
        include_totp: false,
        include_settings: false,
        include_timesheet: false,
    };
    export_json(db, opts)
}

/// Import snippets from **any** snippet-bearing JSON we understand, importing
/// *nothing else*: a full IR backup (only its snippets + groups are applied —
/// history/notes/2FA/settings in the file are ignored), a snippets-only backup,
/// or the lean `[{abbreviation, title?, body}]` / `{"snippets": [...]}` shape
/// (which carries no groups). Encrypted backups are rejected with a clear
/// message — this path is for exchanging snippets, not restoring an app.
///
/// This is what the Snippets tab's file-picker and its drag-and-drop use, so a
/// dropped file "just works" regardless of which of the two formats it is.
pub fn import_snippets_json(db: &DbHandle, json: &str) -> Result<snippets::ImportResult> {
    if is_encrypted(json) {
        return Err(anyhow!(
            "this backup is password-encrypted — import it under Settings → Backup & restore, \
             or re-export it unencrypted"
        ));
    }
    // A backup document is the only shape with a `version` field; the lean
    // snippet shapes are a bare array or a `{snippets: […]}` object.
    let is_backup = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("version").cloned())
        .is_some();
    if !is_backup {
        return snippets::import_from_json(db, json);
    }

    let backup: Backup =
        serde_json::from_str(json).map_err(|e| anyhow!("invalid backup JSON: {e}"))?;
    if backup.version > CURRENT_VERSION {
        return Err(anyhow!(
            "backup version {} is newer than this app supports ({CURRENT_VERSION})",
            backup.version
        ));
    }
    let total = backup.snippets.len();
    let (imported, errors) = apply_snippets(db, &backup);
    Ok(snippets::ImportResult {
        imported,
        skipped: total.saturating_sub(imported),
        errors,
    })
}

fn apply(db: &DbHandle, backup: Backup) -> Result<BackupImportResult> {
    let mut result = BackupImportResult::default();

    // 0+1) Snippet groups + snippets.
    let (imported, errors) = apply_snippets(db, &backup);
    result.snippets_imported = imported;
    result.errors.extend(errors);

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

    // 4) TOTP entries — dedup by (issuer, account, secret), same key the
    //    `totp_import` IPC uses. The table has NO unique constraint (a plain
    //    INSERT always succeeds), so the old "catch the UNIQUE error" branch
    //    was dead code and every restore into a non-empty DB duplicated all
    //    2FA accounts. Present entries count as imported (the desired state
    //    already exists). Secrets are re-encrypted with this machine's key.
    let existing = totp_store::existing_keys(db).unwrap_or_default();
    for (idx, te) in backup.totp_entries.iter().enumerate() {
        if te.issuer.trim().is_empty() || te.secret.trim().is_empty() {
            result
                .errors
                .push(format!("totp #{idx}: empty issuer or secret"));
            continue;
        }
        if existing.contains(&totp_store::dedup_key(&te.issuer, &te.account, &te.secret)) {
            result.totp_imported += 1; // already present — nothing to do
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
                result
                    .errors
                    .push(format!("totp #{idx} ({}): {e}", te.issuer));
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

    // 6) Timesheet (v3, opt-in) — merge with id remapping + dedup so a
    //    re-import creates no duplicates. Titles/urls are re-encrypted with
    //    this machine's key.
    if let Some(ts) = &backup.timesheet {
        match import_timesheet(db, ts) {
            Ok(n) => result.timesheet_imported = n,
            Err(e) => result.errors.push(format!("timesheet: {e}")),
        }
    }

    Ok(result)
}

/// Merge a timesheet backup. Returns the number of events imported (newly
/// inserted OR already present — mirroring the TOTP "desired state exists"
/// counting). Sessions dedup by `started_at`; events by (session, started_at,
/// app, source); claude turns by (event, ts); categories upsert by app.
fn import_timesheet(db: &DbHandle, ts: &TimesheetBackup) -> Result<usize> {
    let conn = db.lock();

    // Sessions — remap backup ids to local ids.
    let mut session_map: HashMap<i64, i64> = HashMap::new();
    for s in &ts.sessions {
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM track_sessions WHERE started_at = ?1 LIMIT 1",
                [s.started_at],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| if e == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(e) })?;
        let local_id = match existing {
            Some(id) => id,
            None => {
                // Never import a session as 'active' — the run loop owns the
                // one live session; a second "active" row would be swept as
                // stale at the next startup anyway. Close it at its last
                // known moment.
                let status = if s.status == "active" { "ended" } else { s.status.as_str() };
                let ended_at = s.ended_at.or_else(|| {
                    ts.events
                        .iter()
                        .filter(|e| e.session_id == s.id)
                        .filter_map(|e| e.ended_at)
                        .max()
                        .or(Some(s.started_at))
                });
                conn.execute(
                    "INSERT INTO track_sessions (label, started_at, ended_at, status) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![s.label, s.started_at, ended_at, status],
                )?;
                conn.last_insert_rowid()
            }
        };
        session_map.insert(s.id, local_id);
    }

    // Events — remap session ids, dedup, re-encrypt title/url.
    let mut imported = 0usize;
    let mut event_map: HashMap<i64, i64> = HashMap::new();
    for e in &ts.events {
        let Some(&session_id) = session_map.get(&e.session_id) else {
            continue; // orphan event (backup inconsistency) — skip silently
        };
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM track_events WHERE session_id = ?1 AND started_at = ?2 \
                 AND app_name = ?3 AND source = ?4 LIMIT 1",
                rusqlite::params![session_id, e.started_at, e.app_name, e.source],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|er| if er == rusqlite::Error::QueryReturnedNoRows { Ok(None) } else { Err(er) })?;
        let local_id = match existing {
            Some(id) => id,
            None => {
                conn.execute(
                    "INSERT INTO track_events (session_id, app_name, app_id, window_title, url, \
                     host, category, project, source, is_idle, started_at, ended_at, duration_s) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    rusqlite::params![
                        session_id,
                        e.app_name,
                        e.app_id,
                        e.window_title.as_deref().map(crypto::encrypt),
                        e.url.as_deref().map(crypto::encrypt),
                        e.host,
                        e.category,
                        e.project,
                        e.source,
                        e.is_idle as i64,
                        e.started_at,
                        e.ended_at,
                        e.duration_s,
                    ],
                )?;
                conn.last_insert_rowid()
            }
        };
        event_map.insert(e.id, local_id);
        imported += 1;
    }

    // Claude turns — dedup by (event, ts).
    for t in &ts.claude_turns {
        let Some(&event_id) = event_map.get(&t.event_id) else { continue };
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM track_claude_turns WHERE event_id = ?1 AND ts = ?2 LIMIT 1",
                rusqlite::params![event_id, t.ts],
                |_| Ok(true),
            )
            .or_else(|er| if er == rusqlite::Error::QueryReturnedNoRows { Ok(false) } else { Err(er) })?;
        if !exists {
            conn.execute(
                "INSERT INTO track_claude_turns (event_id, ts, model, tokens_in, tokens_out) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![event_id, t.ts, t.model, t.tokens_in, t.tokens_out],
            )?;
        }
    }

    // Categories — upsert by app name.
    for c in &ts.categories {
        conn.execute(
            "INSERT OR REPLACE INTO track_categories (app_name, category) VALUES (?1, ?2)",
            rusqlite::params![c.app_name, c.category],
        )?;
    }

    Ok(imported)
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
                note          TEXT,
                derived_from  INTEGER,
                derived_kind  TEXT
            );
            CREATE INDEX idx_last_used ON entries(last_used_at DESC);
            CREATE INDEX idx_hash ON entries(hash);
            "#,
        )
        .unwrap();
        crate::tracking::db::init_schema(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        snippets::init_table(&db).unwrap();
        notes::init_table(&db).unwrap();
        settings::init_table(&db).unwrap();
        totp_store::init_table(&db).unwrap();
        db
    }

    /// Seed one session with two events (one browser event with title+url),
    /// one claude turn and one category rule.
    fn seed_timesheet(db: &DbHandle) {
        let conn = db.lock();
        conn.execute(
            "INSERT INTO track_sessions (label, started_at, ended_at, status) VALUES ('day', 1000, 5000, 'ended')",
            [],
        )
        .unwrap();
        let sid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO track_events (session_id, app_name, source, is_idle, started_at, ended_at, duration_s, window_title, url, host) \
             VALUES (?1, 'Safari', 'browser', 0, 1000, 3000, 2000, ?2, ?3, 'github.com')",
            rusqlite::params![sid, crypto::encrypt("Secret Title"), crypto::encrypt("https://github.com/x")],
        )
        .unwrap();
        let eid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO track_events (session_id, app_name, source, is_idle, started_at, ended_at, duration_s) \
             VALUES (?1, 'iTerm2', 'focus', 0, 3000, 5000, 2000)",
            [sid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO track_claude_turns (event_id, ts, model, tokens_in, tokens_out) VALUES (?1, 1500, 'opus', 10, 20)",
            [eid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO track_categories (app_name, category) VALUES ('iTerm2', 'Dev')",
            [],
        )
        .unwrap();
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
        snippets::create(db, "mfg", "Greeting", "Mit freundlichen Grüßen", None).unwrap();
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
            include_timesheet: false,
        };
        let backup = export(&db, opts).unwrap();
        assert!(backup.history.is_empty(), "history should be empty");
        assert_eq!(backup.snippets.len(), 1, "snippet should be present");
        assert!(backup.notes.is_empty(), "notes should be empty");
    }

    #[test]
    fn snippet_categories_round_trip_by_name() {
        // Export from one machine, import into a fresh machine: groups must
        // survive by NAME and the snippet must land back in its group.
        let src = fresh_db();
        let g = snippets::create_category(&src, "AI Prompts").unwrap();
        snippets::create(&src, "aiplan", "Plan", "body", Some(g)).unwrap();
        snippets::create(&src, "loose", "L", "x", None).unwrap();

        let opts = ExportOptions { include_snippets: true, ..ExportOptions::default() };
        let backup = export(&src, opts).unwrap();
        assert!(backup.snippet_categories.iter().any(|c| c.name == "AI Prompts"));
        let json = serde_json::to_string(&backup).unwrap();

        let dst = fresh_db();
        import_json(&dst, &json).unwrap();
        let all = snippets::list_all(&dst).unwrap();
        assert_eq!(all.iter().find(|s| s.abbreviation == "aiplan").unwrap().category.as_deref(), Some("AI Prompts"));
        assert_eq!(all.iter().find(|s| s.abbreviation == "loose").unwrap().category, None);
        assert!(snippets::list_categories(&dst).unwrap().iter().any(|c| c.name == "AI Prompts"));
    }

    #[test]
    fn empty_category_string_ungroups_a_snippet() {
        // The explicit "ungroup me" sentinel an external editor round-trip
        // needs — `null`/absent must stay "leave the grouping alone".
        let db = fresh_db();
        let g = snippets::create_category(&db, "AI Prompts").unwrap();
        snippets::create(&db, "aiplan", "Plan", "body", Some(g)).unwrap();

        let json = r#"{
            "version": 2,
            "exported_at": 1,
            "snippets": [
                { "id": 0, "abbreviation": "aiplan", "title": "Plan", "body": "body",
                  "created_at": 1, "updated_at": 1, "category": "" }
            ]
        }"#;
        import_json(&db, json).unwrap();

        let s = snippets::find_by_exact_abbreviation(&db, "aiplan").unwrap().unwrap();
        assert_eq!(s.category, None, "empty category string must ungroup");
        // The group itself survives (only the membership was cleared).
        assert!(snippets::list_categories(&db).unwrap().iter().any(|c| c.name == "AI Prompts"));
    }

    #[test]
    fn missing_category_field_preserves_grouping() {
        // The counterpart invariant: a snippet re-imported *without* a category
        // (e.g. from the lean snippet-JSON shape) keeps its existing group.
        let db = fresh_db();
        let g = snippets::create_category(&db, "AI Prompts").unwrap();
        snippets::create(&db, "aiplan", "Plan", "old", Some(g)).unwrap();

        let json = r#"{
            "version": 2,
            "exported_at": 1,
            "snippets": [
                { "id": 0, "abbreviation": "aiplan", "title": "Plan", "body": "new",
                  "created_at": 1, "updated_at": 1 }
            ]
        }"#;
        import_json(&db, json).unwrap();

        let s = snippets::find_by_exact_abbreviation(&db, "aiplan").unwrap().unwrap();
        assert_eq!(s.category.as_deref(), Some("AI Prompts"));
        assert_eq!(s.body, "new", "the body still updates");
    }

    #[test]
    fn snippets_only_export_import_round_trips_groups() {
        // The snippet exchange format: export → import into a fresh DB.
        let src = fresh_db();
        let g = snippets::create_category(&src, "AI Prompts").unwrap();
        snippets::create_category(&src, "Scratch").unwrap(); // deliberately empty
        snippets::create(&src, "aiplan", "Plan", "body", Some(g)).unwrap();
        snippets::create(&src, "loose", "L", "x", None).unwrap();

        let json = export_snippets_json(&src).unwrap();

        let dst = fresh_db();
        let r = import_snippets_json(&dst, &json).unwrap();
        assert_eq!(r.imported, 2);
        assert!(r.errors.is_empty());
        let all = snippets::list_all(&dst).unwrap();
        assert_eq!(
            all.iter().find(|s| s.abbreviation == "aiplan").unwrap().category.as_deref(),
            Some("AI Prompts")
        );
        let cats = snippets::list_categories(&dst).unwrap();
        assert!(cats.iter().any(|c| c.name == "Scratch" && c.count == 0), "empty group survives");
    }

    #[test]
    fn snippet_versions_survive_the_roundtrip_and_reimport_is_a_noop() {
        // Build a snippet at v3, export, import into a fresh DB, and check the
        // version came across; a second import of the same file changes nothing.
        let src = fresh_db();
        let id = snippets::create(&src, "aiplan", "Plan", "body", None).unwrap();
        snippets::update(&src, id, "aiplan", "Plan", "body v2", None).unwrap(); // v2
        snippets::update(&src, id, "aiplan", "Plan v3", "body v2", None).unwrap(); // v3
        assert_eq!(snippets::find_by_exact_abbreviation(&src, "aiplan").unwrap().unwrap().version, 3);

        let json = export_snippets_json(&src).unwrap();
        assert!(json.contains("\"version\": 3"), "export must carry the snippet version");

        let dst = fresh_db();
        import_snippets_json(&dst, &json).unwrap();
        assert_eq!(snippets::find_by_exact_abbreviation(&dst, "aiplan").unwrap().unwrap().version, 3);

        // Re-import the same file → identical content, incoming == local → no bump.
        import_snippets_json(&dst, &json).unwrap();
        assert_eq!(snippets::find_by_exact_abbreviation(&dst, "aiplan").unwrap().unwrap().version, 3);
    }

    #[test]
    fn backup_without_version_field_imports_as_v1() {
        // An old export (no `version` on the snippet) must load as v1.
        let db = fresh_db();
        let json = r#"{
            "version": 2,
            "exported_at": 1,
            "snippets": [
                { "id": 0, "abbreviation": "aa", "title": "T", "body": "b",
                  "created_at": 1, "updated_at": 1 }
            ]
        }"#;
        import_snippets_json(&db, json).unwrap();
        assert_eq!(snippets::find_by_exact_abbreviation(&db, "aa").unwrap().unwrap().version, 1);
    }

    #[test]
    fn backup_merge_takes_the_higher_version_and_bumps_on_content_change() {
        let db = fresh_db();
        // Local snippet at v2.
        let id = snippets::create(&db, "aa", "T", "b", None).unwrap();
        snippets::update(&db, id, "aa", "T", "b2", None).unwrap();
        assert_eq!(snippets::find_by_exact_abbreviation(&db, "aa").unwrap().unwrap().version, 2);

        // Import identical content with a HIGHER incoming version → incoming wins.
        let higher = r#"{"version":2,"exported_at":1,"snippets":[
            {"id":0,"abbreviation":"aa","title":"T","body":"b2","created_at":1,"updated_at":1,"version":7}]}"#;
        import_snippets_json(&db, higher).unwrap();
        assert_eq!(snippets::find_by_exact_abbreviation(&db, "aa").unwrap().unwrap().version, 7);

        // Import DIFFERENT content with a LOWER incoming version → local + 1.
        let lower_diff = r#"{"version":2,"exported_at":1,"snippets":[
            {"id":0,"abbreviation":"aa","title":"T","body":"b3","created_at":1,"updated_at":1,"version":1}]}"#;
        import_snippets_json(&db, lower_diff).unwrap();
        let s = snippets::find_by_exact_abbreviation(&db, "aa").unwrap().unwrap();
        assert_eq!(s.version, 8);
        assert_eq!(s.body, "b3", "the newer content is taken");
    }

    #[test]
    fn snippet_import_of_a_full_backup_touches_only_snippets() {
        // Dropping a FULL app backup on the Snippets tab must import its
        // snippets — and nothing else (no history, no notes, no settings).
        let src = fresh_db();
        seed(&src); // 1 clip + 1 snippet + 1 note
        let json = export_json(&src, ExportOptions::all()).unwrap();

        let dst = fresh_db();
        let r = import_snippets_json(&dst, &json).unwrap();
        assert_eq!(r.imported, 1, "the snippet is imported");
        assert!(db::list(&dst, 100, 0).unwrap().is_empty(), "history untouched");
        assert!(notes::list_all(&dst).unwrap().is_empty(), "notes untouched");
    }

    #[test]
    fn snippet_import_accepts_the_lean_array_shape() {
        let db = fresh_db();
        let r = import_snippets_json(
            &db,
            r#"[{"abbreviation":"hi","title":"Hi","body":"Hallo"}]"#,
        )
        .unwrap();
        assert_eq!(r.imported, 1);
        assert_eq!(
            snippets::find_by_exact_abbreviation(&db, "hi").unwrap().unwrap().body,
            "Hallo"
        );
    }

    #[test]
    fn snippet_import_rejects_an_encrypted_backup() {
        let db = fresh_db();
        seed(&db);
        let enc = export_json_maybe_encrypted(&db, ExportOptions::all(), Some("pw")).unwrap();
        let err = import_snippets_json(&db, &enc).unwrap_err().to_string();
        assert!(err.contains("encrypted"), "{err}");
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
            include_timesheet: false,
        };
        let backup = export(&db, opts).unwrap();
        assert!(backup.history.is_empty());
        assert!(backup.snippets.is_empty());
        assert!(backup.notes.is_empty());
        // Version + timestamp must still be set so the file is parseable.
        // (2, not CURRENT_VERSION: without the timesheet section the file
        // deliberately claims the older, more compatible format.)
        assert_eq!(backup.version, 2);
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
            snippet_categories: vec![],
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
            timesheet: None,
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
        // Timesheet-less exports claim the compatible v2; only a timesheet-
        // carrying file claims CURRENT_VERSION (3).
        let db = fresh_db();
        let json = export_json(&db, ExportOptions::all()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["version"].as_u64().unwrap(), 2);
        let opts = ExportOptions { include_timesheet: true, ..ExportOptions::all() };
        let json = export_json(&db, opts).unwrap();
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

    // ── Timesheet (v3) ───────────────────────────────────────────────

    #[test]
    fn timesheet_excluded_by_default_and_file_stays_v2() {
        let db = fresh_db();
        seed_timesheet(&db);
        let b = export(&db, ExportOptions::all()).unwrap();
        assert_eq!(b.version, 2);
        assert!(b.timesheet.is_none());
        // …and the JSON carries no `timesheet` key at all.
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("timesheet"));
    }

    #[test]
    fn timesheet_roundtrip_decrypts_and_reencrypts() {
        let db = fresh_db();
        seed_timesheet(&db);
        let opts = ExportOptions { include_timesheet: true, ..ExportOptions::all() };
        let b = export(&db, opts).unwrap();
        assert_eq!(b.version, 3);
        let ts = b.timesheet.as_ref().unwrap();
        assert_eq!(ts.sessions.len(), 1);
        assert_eq!(ts.events.len(), 2);
        assert_eq!(ts.claude_turns.len(), 1);
        assert_eq!(ts.categories.len(), 1);
        // Titles/urls are PLAINTEXT in the backup document.
        assert_eq!(ts.events[0].window_title.as_deref(), Some("Secret Title"));
        assert_eq!(ts.events[0].url.as_deref(), Some("https://github.com/x"));

        // Import into a fresh db — everything lands, encrypted at rest again.
        let db2 = fresh_db();
        let json = serde_json::to_string(&b).unwrap();
        let r = import_json(&db2, &json).unwrap();
        assert_eq!(r.timesheet_imported, 2);
        let conn = db2.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM track_sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let stored: String = conn
            .query_row("SELECT window_title FROM track_events WHERE app_name='Safari'", [], |r| r.get(0))
            .unwrap();
        // The import path runs crypto::encrypt on the value. In unit tests the
        // cipher is uninitialized (passthrough), so only the decrypt-roundtrip
        // is asserted; in production this is AES-256-GCM at rest.
        assert_eq!(crypto::decrypt(&stored), "Secret Title");
        let turns: i64 = conn.query_row("SELECT COUNT(*) FROM track_claude_turns", [], |r| r.get(0)).unwrap();
        assert_eq!(turns, 1);
        let cat: String = conn
            .query_row("SELECT category FROM track_categories WHERE app_name='iTerm2'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cat, "Dev");
    }

    #[test]
    fn timesheet_reimport_creates_no_duplicates() {
        let db = fresh_db();
        seed_timesheet(&db);
        let opts = ExportOptions { include_timesheet: true, ..ExportOptions::all() };
        let json = serde_json::to_string(&export(&db, opts).unwrap()).unwrap();
        // Import the backup into the SAME db twice — row counts must not grow.
        for _ in 0..2 {
            import_json(&db, &json).unwrap();
        }
        let conn = db.lock();
        let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM track_sessions", [], |r| r.get(0)).unwrap();
        let events: i64 = conn.query_row("SELECT COUNT(*) FROM track_events", [], |r| r.get(0)).unwrap();
        let turns: i64 = conn.query_row("SELECT COUNT(*) FROM track_claude_turns", [], |r| r.get(0)).unwrap();
        assert_eq!((sessions, events, turns), (1, 2, 1));
    }

    #[test]
    fn imported_active_session_is_closed() {
        // A backup taken mid-session carries status='active' — importing it
        // must never create a second live session on this machine.
        let db = fresh_db();
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO track_sessions (label, started_at, ended_at, status) VALUES (NULL, 7000, NULL, 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO track_events (session_id, app_name, source, is_idle, started_at, ended_at, duration_s) \
                 VALUES (1, 'Xcode', 'focus', 0, 7000, 9000, 2000)",
                [],
            )
            .unwrap();
        }
        let opts = ExportOptions { include_timesheet: true, ..ExportOptions::all() };
        let json = serde_json::to_string(&export(&db, opts).unwrap()).unwrap();
        let db2 = fresh_db();
        import_json(&db2, &json).unwrap();
        let conn = db2.lock();
        let (status, ended): (String, Option<i64>) = conn
            .query_row("SELECT status, ended_at FROM track_sessions", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(status, "ended");
        assert_eq!(ended, Some(9000)); // closed at the last event's end
    }

    #[test]
    fn encrypted_v3_roundtrip() {
        let db = fresh_db();
        seed_timesheet(&db);
        let opts = ExportOptions { include_timesheet: true, ..ExportOptions::all() };
        let enc = export_json_maybe_encrypted(&db, opts, Some("hunter2")).unwrap();
        assert!(is_encrypted(&enc));
        let db2 = fresh_db();
        let r = import_json_maybe_encrypted(&db2, &enc, Some("hunter2")).unwrap();
        assert_eq!(r.timesheet_imported, 2);
        assert!(import_json_maybe_encrypted(&fresh_db(), &enc, Some("wrong")).is_err());
    }
}

