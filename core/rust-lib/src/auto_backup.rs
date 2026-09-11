//! Scheduled local-folder backups (v0.170.0) — the same full export the manual
//! "Backup & restore" writes, but on a timer, into a folder you choose (e.g. a
//! Google Drive / iCloud folder), optionally encrypted, keeping the newest N
//! timestamped snapshots and restorable from Settings.
//!
//! **Why not copy the SQLite file:** the DB is encrypted at rest with a key in
//! this machine's keychain — a raw copy is worthless without exactly that key
//! and never portable. [`crate::backup`] decrypts once and re-encrypts with the
//! user's *backup* password, so a snapshot restores on any machine.
//!
//! Shape mirrors [`crate::device_sync`]: a worker thread on an interval + a
//! debounced wake channel, config + status in the `settings` table, the
//! password in the OS keychain (never in a file), atomic writes (`.tmp` →
//! fsync → rename — right for a folder a cloud client syncs underneath).
//!
//! **Difference from device_sync:** device_sync keeps ONE file per device,
//! overwritten each cycle, for cross-device MERGE. This keeps TIMESTAMPED
//! snapshots with retention, for point-in-time RESTORE. Snapshots use the exact
//! manual-export format, so the existing "Import" button loads them too.

use crate::db::DbHandle;
use crate::{backup, settings};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const KEY_ENABLED: &str = "autobackup.enabled";
pub const KEY_FOLDER: &str = "autobackup.folder";
pub const KEY_INTERVAL_MIN: &str = "autobackup.interval_min";
pub const KEY_KEEP: &str = "autobackup.keep";
pub const KEY_ENCRYPT: &str = "autobackup.encrypt";
pub const KEY_INCLUDE_HISTORY: &str = "autobackup.include_history";
pub const KEY_INCLUDE_TIMESHEET: &str = "autobackup.include_timesheet";
pub const KEY_LAST_MS: &str = "autobackup.last_ms";
pub const KEY_LAST_CHECK_MS: &str = "autobackup.last_check_ms";
pub const KEY_LAST_ERROR: &str = "autobackup.last_error";
/// Hash of the last WRITTEN snapshot's plaintext — lets a cycle skip writing a
/// byte-identical file (a full export every interval would otherwise stack
/// identical multi-MB files in the cloud folder).
pub const KEY_LAST_HASH: &str = "autobackup.last_hash";

/// Keychain slot for the backup password. Same service as the DB key
/// (`crate::crypto`) + device sync; the password is entered per device, never
/// written to a file or the settings table.
const KEYRING_SERVICE: &str = "io.celox.inspector-rust";
const KEYRING_USER: &str = "auto-backup-password-v1";

/// Base worker tick. The configured interval is honoured via [`due`]; the tick
/// only bounds how soon a *changed* interval / a wake is noticed.
const TICK: Duration = Duration::from_secs(30);
const WAKE_DEBOUNCE_MS: u64 = 1500;

const FILE_PREFIX: &str = "inspector-rust-backup-";
/// Encrypted snapshots end `.enc.json`, plaintext `.json`; both end `.json` so
/// the manual "Import" file dialog (which filters JSON) can pick either, and
/// `backup::is_encrypted` decides by CONTENT, not extension.
const ENC_SUFFIX: &str = ".enc.json";
const PLAIN_SUFFIX: &str = ".json";
const TS_FMT: &str = "%Y%m%d-%H%M%S";

pub const DEFAULT_INTERVAL_MIN: u64 = 60;
pub const MIN_INTERVAL_MIN: u64 = 1;
pub const MAX_INTERVAL_MIN: u64 = 10_080; // one week
pub const DEFAULT_KEEP: usize = 24;
pub const MIN_KEEP: usize = 1;
pub const MAX_KEEP: usize = 1000;
/// Don't read anything absurd out of the folder when restoring / peeking.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutoBackupConfig {
    pub enabled: bool,
    /// Absolute path of the destination folder.
    pub folder: String,
    pub interval_min: u64,
    pub keep: usize,
    pub encrypt: bool,
    /// Clipboard history is the size driver (images) — some users don't want it
    /// in the cloud every hour. Everything else (settings, snippets, notes,
    /// 2FA) is always included.
    pub include_history: bool,
    pub include_timesheet: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoBackupStatus {
    /// When the newest snapshot was actually WRITTEN (0 = never).
    pub last_ms: i64,
    /// When the last interval check ran (write or skip).
    pub last_check_ms: i64,
    /// When the next check is due.
    pub next_check_ms: i64,
    pub last_error: String,
    pub encrypt: bool,
    pub has_password: bool,
    pub folder_ok: bool,
    pub snapshot_count: usize,
}

/// One snapshot in the folder, for the restore picker.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SnapshotInfo {
    pub path: String,
    pub name: String,
    /// Timestamp parsed from the filename (ms; local wall clock).
    pub ts_ms: i64,
    pub bytes: u64,
    pub encrypted: bool,
}

/// Outcome of one cycle (for the "Back up now" toast + the worker's bookkeeping).
#[derive(Debug, Default, Clone, Serialize)]
pub struct CycleOutcome {
    pub wrote: bool,
    pub unchanged: bool,
    pub bytes: usize,
    pub pruned: usize,
    pub path: Option<String>,
}

// ── Pure core (unit-tested) ──────────────────────────────────────────────────

pub fn clamp_interval(min: i64) -> u64 {
    (min.max(0) as u64).clamp(MIN_INTERVAL_MIN, MAX_INTERVAL_MIN)
}

pub fn clamp_keep(n: i64) -> usize {
    (n.max(0) as usize).clamp(MIN_KEEP, MAX_KEEP)
}

/// Does the worker do anything? Disabled, no folder, or encryption-on-without-a-
/// password all mean "idle". Encryption OFF runs without a password.
pub fn should_run(cfg: &AutoBackupConfig, has_password: bool) -> bool {
    cfg.enabled && !cfg.folder.trim().is_empty() && (!cfg.encrypt || has_password)
}

/// Is a check due? `last_check_ms == 0` (never run) is always due, so the first
/// backup lands right after you enable it.
pub fn due(last_check_ms: i64, interval_min: u64, now_ms: i64) -> bool {
    if last_check_ms <= 0 {
        return true;
    }
    now_ms - last_check_ms >= interval_min as i64 * 60_000
}

/// Sections to export. 2FA + settings + snippets + notes always on (a backup
/// that can't restore your 2FA or config is not a backup); history + timesheet
/// follow the config.
pub fn export_options(cfg: &AutoBackupConfig) -> backup::ExportOptions {
    backup::ExportOptions {
        include_history: cfg.include_history,
        include_snippets: true,
        include_notes: true,
        include_totp: true,
        include_settings: true,
        include_timesheet: cfg.include_timesheet,
    }
}

pub fn is_our_backup(name: &str) -> bool {
    name.starts_with(FILE_PREFIX) && name.ends_with(PLAIN_SUFFIX)
}

/// Filename for a snapshot at `local` wall-clock time.
pub fn backup_filename(local: chrono::NaiveDateTime, encrypted: bool) -> String {
    let ts = local.format(TS_FMT);
    let suffix = if encrypted { ENC_SUFFIX } else { PLAIN_SUFFIX };
    format!("{FILE_PREFIX}{ts}{suffix}")
}

/// Parse the wall-clock timestamp back out of one of our filenames (ms). `None`
/// when the name isn't ours or the stamp doesn't parse. Inverse of
/// [`backup_filename`] for the time component.
pub fn parse_backup_ts(name: &str) -> Option<i64> {
    if !is_our_backup(name) {
        return None;
    }
    let rest = name.strip_prefix(FILE_PREFIX)?;
    // Strip the more specific suffix first (`.enc.json` also ends `.json`).
    let stamp = rest
        .strip_suffix(ENC_SUFFIX)
        .or_else(|| rest.strip_suffix(PLAIN_SUFFIX))?;
    let dt = chrono::NaiveDateTime::parse_from_str(stamp, TS_FMT).ok()?;
    Some(dt.and_utc().timestamp_millis())
}

pub fn is_encrypted_name(name: &str) -> bool {
    name.ends_with(ENC_SUFFIX)
}

/// Given every one-of-our-backup filename in the folder, the ones to DELETE to
/// keep only the newest `keep`. Names that aren't ours / don't parse are
/// ignored (never returned — a foreign file is never a deletion candidate).
/// Newest = largest parsed timestamp, tie-broken by name so the order can't
/// wobble.
pub fn snapshots_to_delete(names: &[String], keep: usize) -> Vec<String> {
    let mut ours: Vec<(i64, &String)> = names
        .iter()
        .filter_map(|n| parse_backup_ts(n).map(|ts| (ts, n)))
        .collect();
    // Newest first.
    ours.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(a.1)));
    ours.into_iter()
        .skip(keep.max(1))
        .map(|(_, n)| n.clone())
        .collect()
}

fn hash_payload(plaintext: &str) -> String {
    let mut h = Sha256::new();
    h.update(plaintext.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A stable hash of a backup's CONTENT, independent of when it was made and of
/// map iteration order. Zeroes `exported_at` and serialises with every object's
/// keys sorted, so two exports of identical data hash identically (the
/// skip-if-unchanged guarantee). Pure over the value; no I/O.
fn canonical_hash(doc: &backup::Backup) -> Result<String> {
    let mut value = serde_json::to_value(doc).context("backup to value")?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert("exported_at".into(), serde_json::json!(0));
        // ⚠️ The settings table is part of the backup, and a backup WRITES its
        // own heartbeat keys (autobackup.last_hash/last_ms/…). Left in the hash,
        // every export would differ from the previous one and "unchanged" would
        // never match — identical snapshots would pile up hourly, defeating the
        // whole skip. Strip the worker-bookkeeping keys (ours + device-sync +
        // cloud-sync) from the FINGERPRINT only; the written file keeps them.
        if let Some(settings) = obj.get_mut("settings").and_then(|v| v.as_object_mut()) {
            settings.retain(|k, _| !is_volatile_setting(k));
        }
    }
    Ok(hash_payload(&canonicalize(&value)))
}

/// A settings key that changes as a side effect of syncing/backing up, not from
/// user action — excluded from the change fingerprint.
fn is_volatile_setting(key: &str) -> bool {
    matches!(
        key.rsplit('.').next(),
        Some("last_ms" | "last_check_ms" | "last_error" | "last_hash")
    )
}

/// Deterministic string form of a JSON value: object keys sorted recursively,
/// arrays in order. Not valid JSON to re-parse — only a canonical fingerprint.
fn canonicalize(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}={}", k, canonicalize(&map[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canonicalize).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

// ── Config / status / keychain ───────────────────────────────────────────────

/// The default destination: iCloud Drive if present, else none — we never
/// invent a folder.
pub fn default_folder() -> Option<PathBuf> {
    let icloud = dirs::home_dir()?.join("Library/Mobile Documents/com~apple~CloudDocs");
    icloud.is_dir().then(|| icloud.join("InspectorRust-Backups"))
}

pub fn get_config(db: &DbHandle) -> Result<AutoBackupConfig> {
    let folder = settings::get_or(db, KEY_FOLDER, "")?;
    let folder = if folder.trim().is_empty() {
        default_folder()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        folder
    };
    Ok(AutoBackupConfig {
        enabled: settings::get_bool(db, KEY_ENABLED, false)?,
        folder,
        interval_min: clamp_interval(
            settings::get_or(db, KEY_INTERVAL_MIN, "")?
                .parse()
                .unwrap_or(DEFAULT_INTERVAL_MIN as i64),
        ),
        keep: clamp_keep(
            settings::get_or(db, KEY_KEEP, "")?
                .parse()
                .unwrap_or(DEFAULT_KEEP as i64),
        ),
        encrypt: settings::get_bool(db, KEY_ENCRYPT, false)?,
        include_history: settings::get_bool(db, KEY_INCLUDE_HISTORY, true)?,
        include_timesheet: settings::get_bool(db, KEY_INCLUDE_TIMESHEET, false)?,
    })
}

pub fn set_config(db: &DbHandle, cfg: &AutoBackupConfig) -> Result<()> {
    settings::set(db, KEY_ENABLED, bstr(cfg.enabled))?;
    settings::set(db, KEY_FOLDER, cfg.folder.trim())?;
    settings::set(db, KEY_INTERVAL_MIN, &clamp_interval(cfg.interval_min as i64).to_string())?;
    settings::set(db, KEY_KEEP, &clamp_keep(cfg.keep as i64).to_string())?;
    settings::set(db, KEY_ENCRYPT, bstr(cfg.encrypt))?;
    settings::set(db, KEY_INCLUDE_HISTORY, bstr(cfg.include_history))?;
    settings::set(db, KEY_INCLUDE_TIMESHEET, bstr(cfg.include_timesheet))?;
    Ok(())
}

fn bstr(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

pub fn set_password(pass: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .context("keyring entry create failed")?;
    if pass.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(pass).context("keyring set_password failed")
}

pub fn get_password() -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()?
        .get_password()
        .ok()
        .filter(|p| !p.is_empty())
}

pub fn get_status(db: &DbHandle) -> Result<AutoBackupStatus> {
    let cfg = get_config(db)?;
    let dir = PathBuf::from(cfg.folder.trim());
    let snapshot_count = list_snapshots(&cfg.folder).map(|v| v.len()).unwrap_or(0);
    let last_check_ms = settings::get_or(db, KEY_LAST_CHECK_MS, "0")?.parse().unwrap_or(0);
    Ok(AutoBackupStatus {
        last_ms: settings::get_or(db, KEY_LAST_MS, "0")?.parse().unwrap_or(0),
        last_check_ms,
        next_check_ms: if last_check_ms > 0 {
            last_check_ms + cfg.interval_min as i64 * 60_000
        } else {
            0
        },
        last_error: settings::get_or(db, KEY_LAST_ERROR, "")?,
        encrypt: cfg.encrypt,
        has_password: get_password().is_some(),
        folder_ok: dir.is_dir(),
        snapshot_count,
    })
}

/// Every snapshot in `folder`, newest first. Missing/unreadable folder → empty.
pub fn list_snapshots(folder: &str) -> Result<Vec<SnapshotInfo>> {
    let dir = PathBuf::from(folder.trim());
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(Vec::new()),
    };
    let mut out: Vec<SnapshotInfo> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(ts_ms) = parse_backup_ts(&name) else { continue };
        let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(SnapshotInfo {
            path: e.path().to_string_lossy().into_owned(),
            name: name.clone(),
            ts_ms,
            bytes,
            encrypted: is_encrypted_name(&name),
        });
    }
    out.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms).then_with(|| b.name.cmp(&a.name)));
    Ok(out)
}

// ── The cycle (impure edge) ──────────────────────────────────────────────────

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("writing.tmp");
    {
        let mut f = std::fs::File::create(&tmp).context("create tmp failed")?;
        f.write_all(contents.as_bytes()).context("write tmp failed")?;
        f.sync_all().context("fsync tmp failed")?;
    }
    std::fs::rename(&tmp, path).context("rename failed")?;
    Ok(())
}

/// Run one backup. `force` (the "Back up now" button) writes even if nothing
/// changed; the worker passes `false` so an unchanged interval writes nothing.
/// Updates `KEY_LAST_HASH` only on an actual write. Does NOT touch the
/// last_check / last_ms bookkeeping — the caller owns that.
pub fn cycle(
    db: &DbHandle,
    cfg: &AutoBackupConfig,
    password: Option<&str>,
    force: bool,
    now_ms: i64,
) -> Result<CycleOutcome> {
    let dir = PathBuf::from(cfg.folder.trim());
    if !dir.is_dir() {
        anyhow::bail!("backup folder does not exist: {}", cfg.folder);
    }
    if cfg.encrypt && password.map(|p| p.is_empty()).unwrap_or(true) {
        anyhow::bail!("encryption is on but no backup password is set");
    }

    let doc = backup::export(db, export_options(cfg))?;
    // Hash the CONTENT, not the file: `exported_at` is a wall-clock stamp that
    // changes every run, and `settings` is a HashMap whose serialization order
    // isn't stable — hashing the raw JSON would make "unchanged" never match,
    // so identical snapshots would pile up every interval anyway. Canonicalise.
    let hash = canonical_hash(&doc)?;
    let last_hash = settings::get_or(db, KEY_LAST_HASH, "")?;
    if !force && hash == last_hash {
        return Ok(CycleOutcome {
            unchanged: true,
            ..Default::default()
        });
    }
    let plain = serde_json::to_string(&doc).context("serialize backup")?;

    let payload = if cfg.encrypt {
        backup::encrypt_backup(&plain, password.unwrap_or_default())?
    } else {
        plain
    };

    let local = chrono::DateTime::from_timestamp_millis(now_ms)
        .map(|dt| dt.with_timezone(&chrono::Local).naive_local())
        .unwrap_or_else(|| chrono::Local::now().naive_local());
    let name = backup_filename(local, cfg.encrypt);
    let path = dir.join(&name);
    write_atomic(&path, &payload)?;
    settings::set(db, KEY_LAST_HASH, &hash)?;

    // Prune to the newest `keep`, our files only.
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let mut pruned = 0usize;
    for victim in snapshots_to_delete(&names, cfg.keep) {
        if std::fs::remove_file(dir.join(&victim)).is_ok() {
            pruned += 1;
        }
    }

    Ok(CycleOutcome {
        wrote: true,
        unchanged: false,
        bytes: payload_len(&path),
        pruned,
        path: Some(path.to_string_lossy().into_owned()),
    })
}

fn payload_len(path: &Path) -> usize {
    std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0)
}

/// Restore a snapshot file into the live DB. `replace` = wipe-then-apply (a true
/// point-in-time restore); otherwise merge (additive, never deletes). Encrypted
/// files need `password`.
pub fn restore(db: &DbHandle, path: &str, password: Option<&str>, replace: bool) -> Result<backup::BackupImportResult> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {path}"))?;
    if meta.len() > MAX_FILE_BYTES {
        anyhow::bail!("backup file is implausibly large ({} bytes)", meta.len());
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    let json = if backup::is_encrypted(&raw) {
        let pw = password.filter(|p| !p.is_empty()).context("backup is encrypted — password required")?;
        backup::decrypt_backup(&raw, pw)?
    } else {
        raw
    };
    if replace {
        let b: backup::Backup =
            serde_json::from_str(&json).context("invalid backup JSON")?;
        backup::replace_all(db, b)
    } else {
        backup::import_json(db, &json)
    }
}

// ── Worker ───────────────────────────────────────────────────────────────────

static WAKER: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
use parking_lot::Mutex;

/// Nudge the worker to re-check now (after a config change / "Back up now").
pub fn request_check() {
    if let Some(tx) = WAKER.get() {
        let _ = tx.lock().send(());
    }
}

pub fn start(app: AppHandle, db: DbHandle) {
    let (tx, rx): (Sender<()>, Receiver<()>) = std::sync::mpsc::channel();
    let _ = WAKER.set(Mutex::new(tx));
    std::thread::Builder::new()
        .name("ir-auto-backup".into())
        .spawn(move || loop {
            let woken = rx.recv_timeout(TICK).is_ok();
            if woken {
                std::thread::sleep(Duration::from_millis(WAKE_DEBOUNCE_MS));
                while rx.try_recv().is_ok() {}
            }
            let Ok(cfg) = get_config(&db) else { continue };
            let pass = get_password();
            if !should_run(&cfg, pass.is_some()) {
                continue;
            }
            let now = chrono::Utc::now().timestamp_millis();
            let last_check = settings::get_or(&db, KEY_LAST_CHECK_MS, "0")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if !due(last_check, cfg.interval_min, now) {
                continue;
            }
            match cycle(&db, &cfg, pass.as_deref(), false, now) {
                Ok(outcome) => {
                    let _ = settings::set(&db, KEY_LAST_CHECK_MS, &now.to_string());
                    let _ = settings::set(&db, KEY_LAST_ERROR, "");
                    if outcome.wrote {
                        let _ = settings::set(&db, KEY_LAST_MS, &now.to_string());
                    }
                }
                Err(e) => {
                    let _ = settings::set(&db, KEY_LAST_ERROR, &format!("{e:#}"));
                }
            }
            let _ = app.emit("auto-backup-status-changed", ());
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(folder: &str) -> AutoBackupConfig {
        AutoBackupConfig {
            enabled: true,
            folder: folder.into(),
            interval_min: 60,
            keep: 24,
            encrypt: false,
            include_history: true,
            include_timesheet: false,
        }
    }

    #[test]
    fn clamps_hold_the_ranges() {
        assert_eq!(clamp_interval(0), MIN_INTERVAL_MIN);
        assert_eq!(clamp_interval(-5), MIN_INTERVAL_MIN);
        assert_eq!(clamp_interval(999_999), MAX_INTERVAL_MIN);
        assert_eq!(clamp_interval(45), 45);
        assert_eq!(clamp_keep(0), MIN_KEEP);
        assert_eq!(clamp_keep(10_000), MAX_KEEP);
        assert_eq!(clamp_keep(24), 24);
    }

    #[test]
    fn should_run_needs_enabled_folder_and_password_only_when_encrypting() {
        assert!(should_run(&cfg("/x"), false), "unencrypted needs no password");
        let mut c = cfg("/x");
        c.enabled = false;
        assert!(!should_run(&c, true));
        let mut c = cfg("   ");
        assert!(!should_run(&c, true), "blank folder never runs");
        c = cfg("/x");
        c.encrypt = true;
        assert!(!should_run(&c, false), "encrypt-on needs a password");
        assert!(should_run(&c, true));
    }

    #[test]
    fn due_is_immediate_on_first_run_then_interval_gated() {
        assert!(due(0, 60, 1_000), "never-run is always due");
        let now = 10_000_000i64;
        assert!(!due(now - 30 * 60_000, 60, now), "30 min into a 60 min interval → not due");
        assert!(due(now - 60 * 60_000, 60, now), "exactly one interval → due");
        assert!(due(now - 120 * 60_000, 60, now));
    }

    #[test]
    fn filename_roundtrips_and_flags_encryption() {
        let dt = chrono::NaiveDateTime::parse_from_str("20260909-142530", TS_FMT).unwrap();
        let plain = backup_filename(dt, false);
        let enc = backup_filename(dt, true);
        assert_eq!(plain, "inspector-rust-backup-20260909-142530.json");
        assert_eq!(enc, "inspector-rust-backup-20260909-142530.enc.json");
        assert!(is_our_backup(&plain) && is_our_backup(&enc));
        assert!(!is_encrypted_name(&plain) && is_encrypted_name(&enc));
        // The parsed time matches for both, and equals the source instant.
        let want = dt.and_utc().timestamp_millis();
        assert_eq!(parse_backup_ts(&plain), Some(want));
        assert_eq!(parse_backup_ts(&enc), Some(want));
    }

    #[test]
    fn foreign_files_are_never_ours_and_never_pruned() {
        for n in [
            "notes.txt",
            "inspector-rust-backup-.json",       // no timestamp
            "inspector-rust-backup-bad.json",    // unparseable stamp
            "ir-abcd.irsync",                    // device-sync file
            "backup-20260101-000000.json",       // wrong prefix
        ] {
            assert!(parse_backup_ts(n).is_none(), "{n} must not parse");
        }
        // is_our_backup must reject a wrong-prefix .json directly (parse_ts has
        // its own guard, so without this the prefix check here is untested).
        assert!(!is_our_backup("backup-20260101-000000.json"));
        assert!(!is_our_backup("notes.txt"));
        assert!(is_our_backup("inspector-rust-backup-20260101-000000.json"));
        // A folder of mixed files: only our two are deletion candidates, and
        // with keep=1 only the OLDER of the two.
        let names = vec![
            "notes.txt".to_string(),
            "ir-abcd.irsync".to_string(),
            "inspector-rust-backup-20260101-000000.json".to_string(),
            "inspector-rust-backup-20260102-000000.enc.json".to_string(),
        ];
        let del = snapshots_to_delete(&names, 1);
        assert_eq!(del, vec!["inspector-rust-backup-20260101-000000.json".to_string()]);
    }

    #[test]
    fn retention_keeps_the_newest_n_only() {
        let names: Vec<String> = (1..=5)
            .map(|d| format!("inspector-rust-backup-2026010{d}-000000.json"))
            .collect();
        let del = snapshots_to_delete(&names, 2);
        // Keep day 5 + day 4; delete 3, 2, 1.
        assert_eq!(
            del,
            vec![
                "inspector-rust-backup-20260103-000000.json".to_string(),
                "inspector-rust-backup-20260102-000000.json".to_string(),
                "inspector-rust-backup-20260101-000000.json".to_string(),
            ]
        );
        assert!(snapshots_to_delete(&names, 5).is_empty(), "keep>=count deletes nothing");
        assert!(snapshots_to_delete(&names, 99).is_empty());
    }

    // ── Integration: real files in a temp dir, real DB round-trip ──────────
    use crate::db;
    use crate::models::{ContentType, NewClip};
    use parking_lot::Mutex as PlMutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn fresh_db() -> DbHandle {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content_type TEXT NOT NULL, content_text TEXT, content_data BLOB,
                hash TEXT NOT NULL UNIQUE, byte_size INTEGER NOT NULL,
                created_at INTEGER NOT NULL, last_used_at INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0, note TEXT,
                derived_from INTEGER, derived_kind TEXT
            );
            CREATE INDEX idx_last_used ON entries(last_used_at DESC);
            CREATE INDEX idx_hash ON entries(hash);
            "#,
        )
        .unwrap();
        crate::tracking::db::init_schema(&conn).unwrap();
        let db = Arc::new(PlMutex::new(conn));
        crate::snippets::init_table(&db).unwrap();
        crate::notes::init_table(&db).unwrap();
        settings::init_table(&db).unwrap();
        crate::totp_store::init_table(&db).unwrap();
        db
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "ir-autobackup-{}-{}-{}",
            tag,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn seed(db: &DbHandle) {
        db::upsert_clip(
            db,
            &NewClip {
                content_type: ContentType::Text,
                content_text: "hallo welt".into(),
                content_data: "hallo welt".into(),
                byte_size: 10,
            },
        )
        .unwrap();
        crate::snippets::create(db, "mfg", "Gruss", "Mit freundlichen Grüßen", None).unwrap();
    }

    #[test]
    fn volatile_settings_are_recognised() {
        assert!(is_volatile_setting("autobackup.last_hash"));
        assert!(is_volatile_setting("autobackup.last_ms"));
        assert!(is_volatile_setting("devicesync.last_error"));
        assert!(is_volatile_setting("sync.last_check_ms"));
        assert!(!is_volatile_setting("autobackup.interval_min"));
        assert!(!is_volatile_setting("popup.hotkey"));
    }

    #[test]
    fn canonical_hash_ignores_timestamp_and_map_order() {
        use serde_json::json;
        // Two "values" that differ ONLY in exported_at and in object key order
        // must canonicalise to the same string (→ same hash → skip).
        let a = json!({"exported_at": 111, "settings": {"z": "1", "a": "2"}, "history": []});
        let b = json!({"exported_at": 999, "settings": {"a": "2", "z": "1"}, "history": []});
        let mut a2 = a.clone();
        let mut b2 = b.clone();
        a2.as_object_mut().unwrap().insert("exported_at".into(), json!(0));
        b2.as_object_mut().unwrap().insert("exported_at".into(), json!(0));
        assert_eq!(canonicalize(&a2), canonicalize(&b2));
        // A real content difference DOES change it.
        let c = json!({"exported_at": 0, "settings": {"a": "2", "z": "1"}, "history": [1]});
        assert_ne!(canonicalize(&a2), canonicalize(&c));
    }

    #[test]
    fn cycle_writes_a_restorable_snapshot_prunes_and_keeps_newest() {
        let db = fresh_db();
        seed(&db);
        let dir = tmpdir("write");
        let mut c = cfg(&dir.to_string_lossy());
        c.keep = 2;
        // Three forced cycles, one minute apart (distinct second-resolution names).
        let base = chrono::Utc::now().timestamp_millis();
        for k in 0..3 {
            let out = cycle(&db, &c, None, true, base + k * 60_000).unwrap();
            assert!(out.wrote, "forced cycle must write");
        }
        // keep=2 → the oldest of the three was pruned.
        let snaps = list_snapshots(&c.folder).unwrap();
        assert_eq!(snaps.len(), 2, "retention keeps newest 2");
        assert!(snaps[0].ts_ms > snaps[1].ts_ms, "newest first");
        assert!(!snaps[0].encrypted);

        // Restore the newest into a fresh DB — the data comes back.
        let dst = fresh_db();
        let r = restore(&dst, &snaps[0].path, None, false).unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(crate::snippets::list_all(&dst).unwrap()[0].abbreviation, "mfg");
        assert_eq!(db::list(&dst, 10, 0).unwrap()[0].content_text, "hallo welt");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unchanged_interval_writes_nothing() {
        let db = fresh_db();
        seed(&db);
        let dir = tmpdir("unchanged");
        let c = cfg(&dir.to_string_lossy());
        let base = chrono::Utc::now().timestamp_millis();
        // First worker-style cycle (force=false) writes and records the hash.
        assert!(cycle(&db, &c, None, false, base).unwrap().wrote);
        // Second, nothing changed → skipped, no second file.
        let out = cycle(&db, &c, None, false, base + 60_000).unwrap();
        assert!(out.unchanged && !out.wrote);
        assert_eq!(list_snapshots(&c.folder).unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_encrypted_snapshot_round_trips_with_the_password_and_refuses_without() {
        let db = fresh_db();
        seed(&db);
        let dir = tmpdir("enc");
        let mut c = cfg(&dir.to_string_lossy());
        c.encrypt = true;
        let out = cycle(&db, &c, Some("s3cret-pass"), true, chrono::Utc::now().timestamp_millis()).unwrap();
        assert!(out.wrote);
        let snaps = list_snapshots(&c.folder).unwrap();
        assert_eq!(snaps.len(), 1);
        assert!(snaps[0].encrypted, "the .enc.json name + content are encrypted");

        // Wrong / missing password is refused; the right one restores.
        let dst = fresh_db();
        assert!(restore(&dst, &snaps[0].path, None, false).is_err(), "no password → refused");
        assert!(restore(&dst, &snaps[0].path, Some("wrong"), false).is_err(), "wrong password → refused");
        let r = restore(&dst, &snaps[0].path, Some("s3cret-pass"), false).unwrap();
        assert!(r.errors.is_empty());
        assert_eq!(crate::snippets::list_all(&dst).unwrap()[0].abbreviation, "mfg");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replace_mode_wipes_then_applies() {
        let src = fresh_db();
        seed(&src);
        let dir = tmpdir("replace");
        let c = cfg(&dir.to_string_lossy());
        cycle(&src, &c, None, true, chrono::Utc::now().timestamp_millis()).unwrap();
        let snap = &list_snapshots(&c.folder).unwrap()[0].path.clone();

        // A destination with DIFFERENT data; replace must remove it.
        let dst = fresh_db();
        crate::snippets::create(&dst, "old", "Old", "gone after replace", None).unwrap();
        restore(&dst, snap, None, true).unwrap();
        let abbrevs: Vec<String> = crate::snippets::list_all(&dst)
            .unwrap()
            .into_iter()
            .map(|s| s.abbreviation)
            .collect();
        assert!(abbrevs.contains(&"mfg".to_string()));
        assert!(!abbrevs.contains(&"old".to_string()), "replace wiped the pre-existing snippet");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_options_always_include_the_essentials_history_follows_config() {
        let mut c = cfg("/x");
        c.include_history = false;
        c.include_timesheet = true;
        let o = export_options(&c);
        assert!(o.include_settings && o.include_snippets && o.include_notes && o.include_totp);
        assert!(!o.include_history);
        assert!(o.include_timesheet);
    }
}
