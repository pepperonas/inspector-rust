//! Device sync — the same data on several Macs, over a **shared folder**
//! (v0.139.0).
//!
//! Each device publishes ONE file into the folder — its own full export,
//! encrypted — and reads every OTHER device's file on each cycle. No server,
//! no ports, no account: iCloud Drive (or Dropbox, or a USB stick) moves the
//! bytes. Mirrors the shape of [`crate::sync`] (the cue snippet sync): a
//! worker thread, an interval plus a debounced wake, config + status in the
//! `settings` table.
//!
//! # The rule that outranks every other rule
//!
//! **An empty or incomplete state must never overwrite a populated one.**
//! Four independent mechanisms enforce that, in this order:
//!
//! 1. **Merge-only, structurally.** Applying a peer file goes through
//!    [`crate::backup::apply`], which only ever upserts and appends — there is
//!    no delete path in this module at all. Merging an empty document is a
//!    no-op *by construction*, not by a check that could be forgotten. This is
//!    why deletions deliberately do NOT propagate (see *Limits* below): the
//!    moment they did, this guarantee would rest on a condition instead of on
//!    the absence of the code.
//! 2. **Publish gate.** Before replacing our own published file we read the
//!    previous one back and count it; publishing an empty payload over a
//!    populated file is refused ([`publish_verdict`]). An *unreadable*
//!    previous file also refuses — we cannot prove it is worthless, so we fail
//!    closed.
//! 3. **Atomic write.** The new file is written to `.tmp`, fsynced, and
//!    `rename`d over the target (atomic within a filesystem), with the
//!    previous version kept as `.bak`. A reader never observes a half-written
//!    file, and an aborted write leaves the old one untouched.
//! 4. **All-or-nothing read.** A peer file that fails to decrypt or parse is
//!    skipped with a recorded error. Application happens only after a
//!    successful full parse, so a truncated transfer applies nothing.
//!
//! # Limits (deliberate, v1)
//!
//! * **Deletions do not travel.** Devices converge on the UNION of their data.
//!   Deleting a clip on one Mac does not delete it on the other; the next
//!   cycle may even bring it back. That is the price of mechanism 1, and it is
//!   the right trade for a first version — a delete channel is how sync
//!   features destroy data.
//! * **Settings and timesheet data are never synced.** Hotkeys, monitor
//!   brightness, the sync config itself and machine-bound telemetry are local
//!   by nature; syncing the settings table could also overwrite this very
//!   feature's configuration mid-cycle.
//! * **2FA secrets are opt-in** (`devicesync.include_totp`, default off).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::backup::{self, Backup, ExportOptions};
use crate::db::DbHandle;
use crate::notes;
use crate::settings;

pub const KEY_ENABLED: &str = "devicesync.enabled";
pub const KEY_FOLDER: &str = "devicesync.folder";
pub const KEY_DEVICE_ID: &str = "devicesync.device_id";
pub const KEY_INCLUDE_TOTP: &str = "devicesync.include_totp";
pub const KEY_LAST_MS: &str = "devicesync.last_ms";
pub const KEY_LAST_ERROR: &str = "devicesync.last_error";
/// Hash of the last payload we published — lets a cycle skip a pointless
/// rewrite (every encryption draws a fresh salt, so the bytes would differ
/// even when the data didn't, and iCloud would sync the churn).
pub const KEY_LAST_HASH: &str = "devicesync.last_hash";

/// Keychain slot for the shared passphrase. Same service as the DB key
/// (`crate::crypto`); the passphrase is per-device-entered, never synced.
const KEYRING_SERVICE: &str = "io.celox.inspector-rust";
const KEYRING_USER: &str = "device-sync-passphrase-v1";

const INTERVAL_SECS: u64 = 120;
const WAKE_DEBOUNCE_MS: u64 = 2500;
const FILE_PREFIX: &str = "ir-";
const FILE_EXT: &str = ".irsync";
/// Refuse to read anything absurd out of the shared folder.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

// ── Config / status ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncConfig {
    pub enabled: bool,
    /// Absolute path of the shared folder.
    pub folder: String,
    pub include_totp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncStatus {
    pub last_ms: i64,
    pub last_error: String,
    pub device_id: String,
    /// Other devices' files currently in the folder.
    pub peers: usize,
    /// Whether a passphrase is stored in this device's keychain.
    pub has_passphrase: bool,
    /// Whether the configured folder exists and is writable.
    pub folder_ok: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncStats {
    pub peers_read: usize,
    pub clips: usize,
    pub snippets: usize,
    pub notes: usize,
    pub totp: usize,
    pub published: bool,
    /// Human-readable reasons a peer file or the publish step was skipped.
    pub skipped: Vec<String>,
}

/// The default shared folder: iCloud Drive, if this Mac has it. `None` means
/// the user has to pick a folder — we never invent one.
pub fn default_folder() -> Option<PathBuf> {
    let icloud = dirs::home_dir()?
        .join("Library/Mobile Documents/com~apple~CloudDocs");
    icloud
        .is_dir()
        .then(|| icloud.join("InspectorRust-Sync"))
}

pub fn get_config(db: &DbHandle) -> Result<DeviceSyncConfig> {
    let folder = settings::get_or(db, KEY_FOLDER, "")?;
    let folder = if folder.trim().is_empty() {
        default_folder()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        folder
    };
    Ok(DeviceSyncConfig {
        enabled: settings::get_bool(db, KEY_ENABLED, false)?,
        folder,
        include_totp: settings::get_bool(db, KEY_INCLUDE_TOTP, false)?,
    })
}

pub fn set_config(db: &DbHandle, cfg: &DeviceSyncConfig) -> Result<()> {
    settings::set(db, KEY_ENABLED, if cfg.enabled { "true" } else { "false" })?;
    settings::set(db, KEY_FOLDER, cfg.folder.trim())?;
    settings::set(
        db,
        KEY_INCLUDE_TOTP,
        if cfg.include_totp { "true" } else { "false" },
    )?;
    Ok(())
}

/// This device's stable id, generated once and kept in the settings table.
pub fn device_id(db: &DbHandle) -> Result<String> {
    let existing = settings::get_or(db, KEY_DEVICE_ID, "")?;
    if !existing.trim().is_empty() {
        return Ok(existing);
    }
    let id = new_device_id();
    settings::set(db, KEY_DEVICE_ID, &id)?;
    Ok(id)
}

fn new_device_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── Passphrase (keychain, per device) ────────────────────────────────────────

pub fn set_passphrase(pass: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .context("keyring entry create failed")?;
    if pass.is_empty() {
        // Deleting is how the user turns encryption off — which we do not
        // allow, so an empty passphrase simply removes the stored one and the
        // sync stops running (see `should_run`).
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry
        .set_password(pass)
        .context("keyring set_password failed")
}

pub fn get_passphrase() -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()?
        .get_password()
        .ok()
        .filter(|p| !p.is_empty())
}

// ── Pure decisions ───────────────────────────────────────────────────────────

/// Is this a sync file we should consider reading?
pub fn is_sync_file(name: &str) -> bool {
    name.starts_with(FILE_PREFIX) && name.ends_with(FILE_EXT) && name.len() > FILE_PREFIX.len() + FILE_EXT.len()
}

pub fn own_file_name(device_id: &str) -> String {
    format!("{FILE_PREFIX}{device_id}{FILE_EXT}")
}

/// The peer files in a directory listing — every sync file that isn't ours.
/// Pure so the filtering is tested without a filesystem.
pub fn peer_files<'a>(names: &'a [String], own: &str) -> Vec<&'a String> {
    names
        .iter()
        .filter(|n| is_sync_file(n) && n.as_str() != own)
        .collect()
}

/// What we know about the file we are about to replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrevState {
    /// No file published yet.
    None,
    /// Readable, holding this many items.
    Items(usize),
    /// Present but undecryptable/unparseable.
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Publish,
    /// Would replace a populated file with an empty one.
    RefuseEmptyOverPopulated,
    /// Previous file cannot be read, so we cannot prove it is worthless.
    RefuseUnreadablePrevious,
}

/// **The empty-overwrite gate.** Carrying real data always wins; publishing
/// nothing is only allowed when there is demonstrably nothing to lose.
pub fn publish_verdict(new_items: usize, prev: PrevState) -> Verdict {
    if new_items > 0 {
        return Verdict::Publish;
    }
    match prev {
        PrevState::None => Verdict::Publish,
        PrevState::Items(0) => Verdict::Publish,
        PrevState::Items(_) => Verdict::RefuseEmptyOverPopulated,
        PrevState::Unreadable => Verdict::RefuseUnreadablePrevious,
    }
}

/// How many items a document carries across the sections we sync.
pub fn payload_items(b: &Backup) -> usize {
    b.history.len() + b.snippets.len() + b.notes.len() + b.totp_entries.len()
}

/// May the worker do anything at all? Disabled, no folder or no passphrase
/// each mean: touch nothing — no thread work, no file access.
pub fn should_run(cfg: &DeviceSyncConfig, has_passphrase: bool) -> bool {
    cfg.enabled && has_passphrase && !cfg.folder.trim().is_empty()
}

/// Identity of a note for dedup purposes.
///
/// ⚠️ Needed because [`crate::backup::apply`] appends notes **verbatim, with
/// no dedup** — a documented choice for a one-shot restore, but in a repeating
/// sync it would duplicate every note on every cycle.
pub fn note_key(n: &notes::Note) -> (String, String, String) {
    (
        n.title.trim().to_lowercase(),
        n.category.trim().to_lowercase(),
        n.content_text.trim().to_string(),
    )
}

/// The content identity of a snippet — the same three fields
/// [`crate::snippets::upsert_by_abbreviation`] compares.
fn snippet_content(s: &crate::snippets::Snippet) -> (String, String, String) {
    (s.abbreviation.clone(), s.title.clone(), s.body.clone())
}

/// Decide, per incoming snippet, whether it may reach the merge.
///
/// ⚠️ **This exists because [`crate::snippets::merge_version`] has no
/// "local is newer" branch**: on differing content the incoming side always
/// wins (with a `local + 1` bump). For the cue sync that is correct and
/// deliberate — cue is declared the master. Between two equal peers it is data
/// loss: syncing a peer's stale copy silently REVERTED a local edit and bumped
/// the version so the revert looked authoritative (observed, then pinned).
///
/// The rule here is symmetric and converges in one round:
/// * no local counterpart → adopt
/// * incoming version higher → adopt
/// * incoming version lower → drop, keep the local edit
/// * equal versions, identical content → drop (nothing to do)
/// * equal versions, different content → the lexicographically greater
///   content wins. Deterministic, so BOTH devices reach the same answer —
///   a "newest write wins" tie-break would ping-pong forever.
pub fn filter_snippets(
    incoming: Vec<crate::snippets::Snippet>,
    local: &[crate::snippets::Snippet],
) -> Vec<crate::snippets::Snippet> {
    incoming
        .into_iter()
        .filter(|inc| {
            let Some(loc) = local.iter().find(|l| l.abbreviation == inc.abbreviation) else {
                return true;
            };
            match inc.version.cmp(&loc.version) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => snippet_content(inc) > snippet_content(loc),
            }
        })
        .collect()
}

/// Drop the incoming notes we already have. Pure.
pub fn dedup_notes(
    incoming: Vec<notes::Note>,
    existing: &HashSet<(String, String, String)>,
) -> Vec<notes::Note> {
    let mut seen = existing.clone();
    let mut out = Vec::new();
    for n in incoming {
        let k = note_key(&n);
        if seen.insert(k) {
            out.push(n);
        }
    }
    out
}

// ── Filesystem I/O ───────────────────────────────────────────────────────────

/// Export exactly the sections this feature syncs. Settings and timesheet are
/// excluded on purpose (see the module docs).
fn export_options(cfg: &DeviceSyncConfig) -> ExportOptions {
    ExportOptions {
        include_history: true,
        include_snippets: true,
        include_notes: true,
        include_totp: cfg.include_totp,
        include_settings: false,
        include_timesheet: false,
    }
}

/// Read + decrypt + parse one file. Any failure is an error — never a partial
/// application (mechanism 4).
pub fn read_file(path: &Path, pass: &str) -> Result<Backup> {
    let meta = std::fs::metadata(path).context("stat failed")?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(anyhow!("file too large ({} bytes)", meta.len()));
    }
    let raw = std::fs::read_to_string(path).context("read failed")?;
    let json = backup::decrypt_backup(&raw, pass).context("decrypt failed")?;
    let b: Backup = serde_json::from_str(&json).context("parse failed")?;
    Ok(b)
}

/// Write `contents` to `path` so that a reader sees either the old file or the
/// new one, never a mixture (mechanism 3).
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp).context("create tmp failed")?;
        f.write_all(contents.as_bytes()).context("write tmp failed")?;
        f.sync_all().context("fsync tmp failed")?;
    }
    // Keep the previous good version around before swapping.
    if path.exists() {
        let bak = path.with_extension("bak");
        let _ = std::fs::copy(path, &bak);
    }
    std::fs::rename(&tmp, path).context("rename failed")?;
    Ok(())
}

/// Publish this device's state, honouring the empty-overwrite gate.
fn publish(db: &DbHandle, dir: &Path, own: &str, cfg: &DeviceSyncConfig, pass: &str, stats: &mut SyncStats) -> Result<()> {
    let opts = export_options(cfg);
    let doc = backup::export(db, opts).context("export failed")?;
    let items = payload_items(&doc);

    let target = dir.join(own);
    let prev = if target.exists() {
        match read_file(&target, pass) {
            Ok(b) => PrevState::Items(payload_items(&b)),
            Err(_) => PrevState::Unreadable,
        }
    } else {
        PrevState::None
    };

    match publish_verdict(items, prev) {
        Verdict::Publish => {}
        Verdict::RefuseEmptyOverPopulated => {
            stats.skipped.push(
                "Veröffentlichen abgelehnt: der lokale Stand ist leer, der bereits veröffentlichte nicht."
                    .into(),
            );
            return Ok(());
        }
        Verdict::RefuseUnreadablePrevious => {
            stats.skipped.push(
                "Veröffentlichen abgelehnt: die eigene Datei ist unlesbar und der lokale Stand leer."
                    .into(),
            );
            return Ok(());
        }
    }

    let json = serde_json::to_string(&doc).context("serialize failed")?;
    // Skip a pointless rewrite: encryption draws a fresh salt every time, so
    // unchanged data would still produce different bytes and churn iCloud.
    let hash = crate::db::hash_payload(crate::models::ContentType::Text, &json);
    if settings::get_or(db, KEY_LAST_HASH, "").unwrap_or_default() == hash && target.exists() {
        return Ok(());
    }
    let envelope = backup::encrypt_backup(&json, pass).context("encrypt failed")?;
    write_atomic(&target, &envelope)?;
    let _ = settings::set(db, KEY_LAST_HASH, &hash);
    stats.published = true;
    Ok(())
}

/// Merge one peer document in. Additive only.
fn apply_incoming(db: &DbHandle, mut doc: Backup, stats: &mut SyncStats) -> Result<()> {
    // Never let a peer's settings or timesheet in, whatever the file claims.
    doc.settings.clear();
    doc.timesheet = None;

    let existing: HashSet<_> = notes::list_all(db)
        .unwrap_or_default()
        .iter()
        .map(note_key)
        .collect();
    doc.notes = dedup_notes(std::mem::take(&mut doc.notes), &existing);

    // Keep a peer's stale snippet from reverting a local edit — see
    // `filter_snippets` for why the shared merge rule can't do this itself.
    let local = crate::snippets::list_all(db).unwrap_or_default();
    doc.snippets = filter_snippets(std::mem::take(&mut doc.snippets), &local);

    let res = backup::apply(db, doc).context("merge failed")?;
    stats.clips += res.history_imported;
    stats.snippets += res.snippets_imported;
    stats.notes += res.notes_imported;
    stats.totp += res.totp_imported;
    Ok(())
}

/// One full cycle: read every peer file, then publish our own.
pub fn cycle(db: &DbHandle, cfg: &DeviceSyncConfig, pass: &str) -> Result<SyncStats> {
    let dir = PathBuf::from(cfg.folder.trim());
    std::fs::create_dir_all(&dir).with_context(|| format!("Ordner nicht nutzbar: {}", dir.display()))?;

    let own = own_file_name(&device_id(db)?);
    let mut stats = SyncStats::default();

    let names: Vec<String> = std::fs::read_dir(&dir)
        .context("Ordner nicht lesbar")?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    for name in peer_files(&names, &own) {
        match read_file(&dir.join(name), pass) {
            Ok(doc) => {
                stats.peers_read += 1;
                if let Err(e) = apply_incoming(db, doc, &mut stats) {
                    stats.skipped.push(format!("{name}: {e:#}"));
                }
            }
            // Mechanism 4: unreadable → skipped whole, nothing applied.
            Err(e) => stats.skipped.push(format!("{name}: {e:#}")),
        }
    }

    publish(db, &dir, &own, cfg, pass, &mut stats)?;
    Ok(stats)
}

/// Status for the Settings UI.
pub fn get_status(db: &DbHandle) -> Result<DeviceSyncStatus> {
    let cfg = get_config(db)?;
    let id = device_id(db)?;
    let own = own_file_name(&id);
    let dir = PathBuf::from(cfg.folder.trim());
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    Ok(DeviceSyncStatus {
        last_ms: settings::get_or(db, KEY_LAST_MS, "0")?.parse().unwrap_or(0),
        last_error: settings::get_or(db, KEY_LAST_ERROR, "")?,
        peers: peer_files(&names, &own).len(),
        device_id: id,
        has_passphrase: get_passphrase().is_some(),
        folder_ok: dir.is_dir(),
    })
}

// ── Worker ───────────────────────────────────────────────────────────────────

static WAKER: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

/// Ask for a cycle soon (debounced). No-op until [`start`] ran, and the worker
/// still re-checks the config — so this never starts a sync that is disabled.
pub fn request_sync() {
    if let Some(tx) = WAKER.get() {
        let _ = tx.lock().map(|tx| tx.send(()));
    }
}

pub fn start(app: AppHandle, db: DbHandle) {
    let (tx, rx): (Sender<()>, Receiver<()>) = std::sync::mpsc::channel();
    let _ = WAKER.set(Mutex::new(tx));
    std::thread::Builder::new()
        .name("ir-device-sync".into())
        .spawn(move || loop {
            let woken = rx.recv_timeout(Duration::from_secs(INTERVAL_SECS)).is_ok();
            if woken {
                std::thread::sleep(Duration::from_millis(WAKE_DEBOUNCE_MS));
                while rx.try_recv().is_ok() {}
            }
            let Ok(cfg) = get_config(&db) else { continue };
            let pass = get_passphrase();
            // Disabled → no file access, no export, nothing.
            if !should_run(&cfg, pass.is_some()) {
                continue;
            }
            let pass = pass.unwrap_or_default();
            match cycle(&db, &cfg, &pass) {
                Ok(stats) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let _ = settings::set(&db, KEY_LAST_MS, &now.to_string());
                    let _ = settings::set(
                        &db,
                        KEY_LAST_ERROR,
                        &stats.skipped.first().cloned().unwrap_or_default(),
                    );
                    if stats.clips + stats.snippets + stats.notes + stats.totp > 0 {
                        let _ = app.emit("clipboard-changed", ());
                        let _ = app.emit("snippets-synced", ());
                        let _ = app.emit("device-sync-applied", ());
                    }
                }
                Err(e) => {
                    let _ = settings::set(&db, KEY_LAST_ERROR, &format!("{e:#}"));
                }
            }
            let _ = app.emit("device-sync-status-changed", ());
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ContentType;
    use crate::models::NewClip;
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    /// A device: its own in-memory database.
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
            CREATE INDEX idx_hash ON entries(hash);
            "#,
        )
        .unwrap();
        crate::tracking::db::init_schema(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        crate::snippets::init_table(&db).unwrap();
        notes::init_table(&db).unwrap();
        settings::init_table(&db).unwrap();
        crate::totp_store::init_table(&db).unwrap();
        db
    }

    fn clip(db: &DbHandle, text: &str) {
        crate::db::upsert_clip(
            db,
            &NewClip {
                content_type: ContentType::Text,
                content_text: text.into(),
                content_data: text.into(),
                byte_size: text.len() as i64,
            },
        )
        .unwrap();
    }

    fn clip_count(db: &DbHandle) -> usize {
        crate::db::list(db, 10_000, 0).unwrap().len()
    }

    fn temp_folder() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-dsync-{}", new_device_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn cfg_for(dir: &Path) -> DeviceSyncConfig {
        DeviceSyncConfig {
            enabled: true,
            folder: dir.to_string_lossy().into_owned(),
            include_totp: false,
        }
    }


    fn note(title: &str, body: &str) -> notes::Note {
        notes::Note {
            id: 0,
            content_type: ContentType::Text,
            content_text: body.into(),
            content_data: String::new(),
            title: title.into(),
            category: "Allgemein".into(),
            byte_size: body.len() as i64,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn doc(history: usize, snippets: usize) -> Backup {
        Backup {
            version: backup::CURRENT_VERSION,
            exported_at: 0,
            history: Vec::new(),
            snippets: Vec::new(),
            snippet_categories: Vec::new(),
            notes: Vec::new(),
            totp_entries: Vec::new(),
            settings: Default::default(),
            timesheet: None,
        }
        .tap(history, snippets)
    }

    // Small helper so the fixtures stay readable.
    trait Tap {
        fn tap(self, history: usize, snippets: usize) -> Backup;
    }
    impl Tap for Backup {
        fn tap(mut self, history: usize, snippets: usize) -> Backup {
            for i in 0..history {
                let text = format!("clip {i}");
                self.history.push(crate::models::ClipEntry {
                    id: i as i64 + 1,
                    content_type: ContentType::Text,
                    hash: crate::db::hash_payload(ContentType::Text, &text),
                    content_text: text.clone(),
                    content_data: text,
                    byte_size: 6,
                    created_at: 1,
                    last_used_at: 1,
                    pinned: false,
                    note: None,
                    derived_from: None,
                    derived_kind: None,
                });
            }
            for i in 0..snippets {
                self.snippets.push(crate::snippets::Snippet {
                    id: i as i64 + 1,
                    abbreviation: format!("ab{i}"),
                    title: format!("t{i}"),
                    body: format!("b{i}"),
                    category: None,
                    version: 1,
                    created_at: 1,
                    updated_at: 1,
                });
            }
            self
        }
    }

    // ── The rule that outranks the others ────────────────────────────────

    #[test]
    fn an_empty_payload_never_replaces_a_populated_file() {
        assert_eq!(
            publish_verdict(0, PrevState::Items(42)),
            Verdict::RefuseEmptyOverPopulated
        );
        // …which is exactly the fresh-install-on-a-new-Mac case.
        assert_eq!(publish_verdict(0, PrevState::Items(1)), Verdict::RefuseEmptyOverPopulated);
    }

    #[test]
    fn an_unreadable_previous_file_fails_closed() {
        // We cannot prove it is worthless, so an empty payload must not
        // replace it.
        assert_eq!(
            publish_verdict(0, PrevState::Unreadable),
            Verdict::RefuseUnreadablePrevious
        );
        // Carrying real data is always allowed — overwriting our OWN corrupt
        // file with something good is the desired outcome.
        assert_eq!(publish_verdict(7, PrevState::Unreadable), Verdict::Publish);
    }

    #[test]
    fn publishing_is_allowed_when_there_is_nothing_to_lose() {
        assert_eq!(publish_verdict(0, PrevState::None), Verdict::Publish);
        assert_eq!(publish_verdict(0, PrevState::Items(0)), Verdict::Publish);
        assert_eq!(publish_verdict(5, PrevState::Items(9)), Verdict::Publish);
    }

    #[test]
    fn payload_items_counts_every_synced_section() {
        assert_eq!(payload_items(&doc(0, 0)), 0);
        assert_eq!(payload_items(&doc(3, 2)), 5);
        let mut d = doc(0, 0);
        d.notes.push(note("n", "b"));
        assert_eq!(payload_items(&d), 1);
    }

    // ── Disabled means disabled ──────────────────────────────────────────

    #[test]
    fn a_disabled_switch_runs_nothing() {
        let on = DeviceSyncConfig { enabled: true, folder: "/tmp/x".into(), include_totp: false };
        let off = DeviceSyncConfig { enabled: false, ..on.clone() };
        assert!(should_run(&on, true));
        assert!(!should_run(&off, true));
        // …and neither does a missing passphrase or folder.
        assert!(!should_run(&on, false));
        let no_folder = DeviceSyncConfig { folder: "  ".into(), ..on.clone() };
        assert!(!should_run(&no_folder, true));
    }

    // ── File selection ───────────────────────────────────────────────────

    #[test]
    fn only_our_peers_files_are_read() {
        let own = own_file_name("aabb");
        let names: Vec<String> = vec![
            own.clone(),
            "ir-ccdd.irsync".into(),
            "ir-ccdd.irsync.bak".into(), // a backup copy is not a peer
            "ir-ccdd.tmp".into(),        // an in-flight write is not a peer
            "notes.txt".into(),
            ".DS_Store".into(),
            "ir-.irsync".into(), // no device id
        ];
        let peers = peer_files(&names, &own);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0], "ir-ccdd.irsync");
    }

    #[test]
    fn device_ids_are_distinct_and_hex() {
        let a = new_device_id();
        let b = new_device_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── Notes dedup (the repeating-sync hazard) ──────────────────────────

    #[test]
    fn notes_already_present_are_not_appended_again() {
        let existing: HashSet<_> = [note("Titel", "Inhalt")].iter().map(note_key).collect();
        let incoming = vec![note("Titel", "Inhalt"), note("Neu", "Anderes")];
        let out = dedup_notes(incoming, &existing);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Neu");
    }

    #[test]
    fn a_duplicate_inside_one_document_is_dropped_too() {
        let out = dedup_notes(
            vec![note("A", "x"), note("A", "x"), note("B", "y")],
            &HashSet::new(),
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn the_note_key_ignores_case_and_padding_but_not_content() {
        assert_eq!(note_key(&note(" Titel ", "Inhalt")), note_key(&note("titel", "Inhalt")));
        assert_ne!(note_key(&note("Titel", "a")), note_key(&note("Titel", "b")));
    }

    // ── Snippet conflicts ────────────────────────────────────────────────

    fn snip(abbr: &str, body: &str, version: i64) -> crate::snippets::Snippet {
        crate::snippets::Snippet {
            id: 0,
            abbreviation: abbr.into(),
            title: "t".into(),
            body: body.into(),
            category: None,
            version,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn a_peers_stale_snippet_never_reverts_a_local_edit() {
        // The defect this function exists for: the shared merge rule has no
        // "local is newer" branch, so without the filter the v1 copy below
        // overwrote the local v2 edit AND bumped it to v3.
        let local = vec![snip("sig", "meine Fassung", 2)];
        let kept = filter_snippets(vec![snip("sig", "alte Fassung", 1)], &local);
        assert!(kept.is_empty());
    }

    #[test]
    fn a_newer_peer_snippet_is_adopted_and_an_unknown_one_too() {
        let local = vec![snip("sig", "alt", 2)];
        let kept = filter_snippets(vec![snip("sig", "neu", 3), snip("andere", "x", 1)], &local);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn equal_versions_with_identical_content_are_dropped() {
        let local = vec![snip("sig", "gleich", 2)];
        assert!(filter_snippets(vec![snip("sig", "gleich", 2)], &local).is_empty());
    }

    #[test]
    fn a_tie_is_broken_the_same_way_on_both_devices() {
        // Same version, different content. Whichever device asks, the SAME
        // side must win — otherwise the two keep overwriting each other.
        let a = snip("sig", "alpha", 2);
        let b = snip("sig", "beta", 2);
        let a_adopts_b = !filter_snippets(vec![b.clone()], std::slice::from_ref(&a)).is_empty();
        let b_adopts_a = !filter_snippets(vec![a.clone()], std::slice::from_ref(&b)).is_empty();
        assert!(a_adopts_b ^ b_adopts_a, "genau eine Seite darf nachgeben");
    }

    // ── Atomic write + all-or-nothing read (real files) ──────────────────

    #[test]
    fn an_interrupted_write_leaves_the_previous_file_intact() {
        let dir = std::env::temp_dir().join(format!("ir-ds-{}", new_device_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ir-aabb.irsync");

        write_atomic(&target, "erste fassung").unwrap();
        // Simulate a transfer that died mid-way: a .tmp is left behind and the
        // rename never happened.
        std::fs::write(target.with_extension("tmp"), "halb geschrieb").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "erste fassung");

        // A completed second write swaps atomically and keeps a .bak.
        write_atomic(&target, "zweite fassung").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "zweite fassung");
        assert_eq!(
            std::fs::read_to_string(target.with_extension("bak")).unwrap(),
            "erste fassung"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_concurrent_reader_only_ever_sees_a_complete_version() {
        // ⚠️ HONEST LIMIT: this does NOT prove atomicity. Replacing the
        // `rename` with a plain `fs::write` leaves it green on APFS (measured
        // — 2 MB payloads, 12 swaps, a reader spinning throughout), so the
        // guarantee in mechanism 3 rests on the POSIX rename contract, not on
        // this test. What it does pin is the observable invariant — a reader
        // must never see a prefix — which would catch a future writer that
        // streams into the target in chunks over a longer window.
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = temp_folder();
        let target = dir.join("ir-aabb.irsync");
        let a = "a".repeat(2 * 1024 * 1024);
        let b = "b".repeat(2 * 1024 * 1024);
        write_atomic(&target, &a).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let seen_partial = Arc::new(AtomicBool::new(false));
        let reader = {
            let (t, stop, flag) = (target.clone(), stop.clone(), seen_partial.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(s) = std::fs::read_to_string(&t) {
                        let whole = (s.len() == 2 * 1024 * 1024)
                            && (s.bytes().all(|c| c == b'a') || s.bytes().all(|c| c == b'b'));
                        if !whole {
                            flag.store(true, Ordering::Relaxed);
                        }
                    }
                }
            })
        };
        for i in 0..12 {
            write_atomic(&target, if i % 2 == 0 { &b } else { &a }).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        reader.join().unwrap();
        assert!(
            !seen_partial.load(Ordering::Relaxed),
            "ein Leser hat einen unvollständigen Stand gesehen"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_or_wrongly_keyed_file_is_rejected_whole() {
        let dir = std::env::temp_dir().join(format!("ir-ds-{}", new_device_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ir-ccdd.irsync");

        let json = serde_json::to_string(&doc(2, 1)).unwrap();
        let envelope = backup::encrypt_backup(&json, "geheim").unwrap();
        std::fs::write(&path, &envelope).unwrap();
        assert_eq!(payload_items(&read_file(&path, "geheim").unwrap()), 3);

        // Wrong passphrase → error, never a partial document.
        assert!(read_file(&path, "falsch").is_err());

        // Truncated mid-transfer → error.
        std::fs::write(&path, &envelope[..envelope.len() / 2]).unwrap();
        assert!(read_file(&path, "geheim").is_err());

        // Plain garbage → error.
        std::fs::write(&path, "{}").unwrap();
        assert!(read_file(&path, "geheim").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Cycle-level proofs (two "devices", one real folder) ──────────────

    const PASS: &str = "gemeinsames-geheimnis";

    #[test]
    fn two_devices_converge_through_the_shared_folder() {
        let dir = temp_folder();
        let cfg = cfg_for(&dir);
        let a = fresh_db();
        let b = fresh_db();
        clip(&a, "von A");
        clip(&b, "von B");

        cycle(&a, &cfg, PASS).unwrap(); // A publishes
        let s = cycle(&b, &cfg, PASS).unwrap(); // B reads A, publishes
        assert_eq!(s.peers_read, 1);
        assert_eq!(clip_count(&b), 2, "B kennt jetzt beide");

        cycle(&a, &cfg, PASS).unwrap(); // A reads B
        assert_eq!(clip_count(&a), 2, "A kennt jetzt beide");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repeated_cycle_adds_nothing_twice() {
        let dir = temp_folder();
        let cfg = cfg_for(&dir);
        let a = fresh_db();
        let b = fresh_db();
        clip(&a, "einmalig");
        crate::snippets::create(&a, "ab", "Titel", "Rumpf", None).unwrap();
        notes::create_text(&a, "Notiz", "Inhalt", "Allgemein").unwrap();
        cycle(&a, &cfg, PASS).unwrap();

        for _ in 0..3 {
            cycle(&b, &cfg, PASS).unwrap();
        }
        assert_eq!(clip_count(&b), 1);
        assert_eq!(crate::snippets::list_all(&b).unwrap().len(), 1);
        // The notes dedup is what this really pins: backup::apply appends
        // notes verbatim, so without it three cycles would leave three copies.
        assert_eq!(notes::list_all(&b).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Requirement 3, at the level that matters ─────────────────────────

    #[test]
    fn an_empty_peer_overwrites_nothing() {
        let dir = temp_folder();
        let cfg = cfg_for(&dir);
        let full = fresh_db();
        let empty = fresh_db();
        for i in 0..5 {
            clip(&full, &format!("wichtig {i}"));
        }
        cycle(&empty, &cfg, PASS).unwrap(); // the empty device publishes first
        cycle(&full, &cfg, PASS).unwrap(); // …and the populated one reads it

        assert_eq!(clip_count(&full), 5, "nichts darf verschwunden sein");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wiped_device_does_not_wipe_the_shared_state() {
        // The case the requirement calls out by name: first start on a new
        // machine (or a lost database) while OUR file already holds data.
        let dir = temp_folder();
        let cfg = cfg_for(&dir);
        let dev = fresh_db();
        for i in 0..3 {
            clip(&dev, &format!("bestand {i}"));
        }
        cycle(&dev, &cfg, PASS).unwrap();
        let own = dir.join(own_file_name(&device_id(&dev).unwrap()));
        assert_eq!(payload_items(&read_file(&own, PASS).unwrap()), 3);

        // Same device id, empty database.
        let wiped = fresh_db();
        settings::set(&wiped, KEY_DEVICE_ID, &device_id(&dev).unwrap()).unwrap();
        let stats = cycle(&wiped, &cfg, PASS).unwrap();

        assert!(!stats.published, "leerer Stand darf nicht veröffentlicht werden");
        assert!(stats.skipped.iter().any(|s| s.contains("abgelehnt")));
        assert_eq!(
            payload_items(&read_file(&own, PASS).unwrap()),
            3,
            "die veröffentlichte Datei muss unangetastet bleiben"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_aborted_transfer_leaves_no_damaged_state() {
        let dir = temp_folder();
        let cfg = cfg_for(&dir);
        let a = fresh_db();
        let b = fresh_db();
        clip(&a, "gut");
        cycle(&a, &cfg, PASS).unwrap();

        // A second peer whose file arrived half-written.
        let good = dir.join(own_file_name(&device_id(&a).unwrap()));
        let half = std::fs::read_to_string(&good).unwrap();
        std::fs::write(dir.join("ir-deadbeef.irsync"), &half[..half.len() / 2]).unwrap();

        clip(&b, "eigenes");
        let stats = cycle(&b, &cfg, PASS).unwrap();

        assert_eq!(stats.peers_read, 1, "nur die heile Datei zählt");
        assert!(stats.skipped.iter().any(|s| s.contains("deadbeef")));
        // The good peer still landed, our own data survived, and we published.
        assert_eq!(clip_count(&b), 2);
        assert!(stats.published);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Conflict resolution ──────────────────────────────────────────────

    #[test]
    fn the_higher_snippet_version_wins_in_both_directions() {
        let dir = temp_folder();
        let cfg = cfg_for(&dir);
        let a = fresh_db();
        let b = fresh_db();

        crate::snippets::create(&a, "sig", "A", "Fassung A", None).unwrap();
        cycle(&a, &cfg, PASS).unwrap();
        cycle(&b, &cfg, PASS).unwrap();
        assert_eq!(crate::snippets::list_all(&b).unwrap()[0].body, "Fassung A");

        // B edits → version 2 → A must adopt it.
        let id = crate::snippets::list_all(&b).unwrap()[0].id;
        crate::snippets::update(&b, id, "sig", "B", "Fassung B", None).unwrap();
        cycle(&b, &cfg, PASS).unwrap();
        cycle(&a, &cfg, PASS).unwrap();

        let on_a = crate::snippets::list_all(&a).unwrap();
        assert_eq!(on_a.len(), 1, "kein Duplikat, dieselbe Abkürzung");
        assert_eq!(on_a[0].body, "Fassung B", "die höhere Version gewinnt");
        assert!(on_a[0].version >= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── The switch ───────────────────────────────────────────────────────

    #[test]
    fn a_disabled_switch_touches_not_a_single_file() {
        let dir = temp_folder();
        let db = fresh_db();
        clip(&db, "lokal");
        let off = DeviceSyncConfig { enabled: false, ..cfg_for(&dir) };

        // The worker's own guard — the only thing standing between "disabled"
        // and any file access.
        assert!(!should_run(&off, true));
        if should_run(&off, true) {
            cycle(&db, &off, PASS).unwrap();
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "Ordner blieb unberührt");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
