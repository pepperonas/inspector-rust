//! Cloud sync with cue (<https://cue.celox.io>) — bidirectional snippet sync.
//!
//! IR is the polling client (cue cannot reach the desktop). Every cycle:
//! **pull** `GET {url}/api/sync/snippets` (Bearer token) → apply remote
//! tombstones + snippets locally → **push** the local snippets in scope plus
//! local tombstones to `POST {url}/api/sync/snippets`.
//!
//! The sync SCOPE (which groups participate, incl. the "ungrouped" flag) is
//! managed in cue and arrives with every pull — IR stores no scope config.
//!
//! Merge rule (identical on both sides, deterministic):
//! - `incoming.version > local.version` → adopt incoming content
//! - equal versions, identical content  → no-op (cue's grouping is adopted —
//!   cue is the organizational master)
//! - equal versions, different content  → cue wins (adopt; the resulting +1
//!   bump converges both sides on the next push)
//! - `incoming.version < local.version` → keep local (the push carries ours)
//!
//! Deletions travel as tombstones (see [`crate::snippets`]): a tombstone
//! deletes the local copy only if that copy isn't newer; recreations start
//! above the tombstone's version. Tombstones are pruned after 90 days.
//!
//! Settings keys: `sync.enabled` (bool), `sync.url`, `sync.token`; status in
//! `sync.last_ms` / `sync.last_error`. The worker wakes every
//! [`SYNC_INTERVAL_SECS`] or immediately via [`request_sync`] (called after
//! every snippet mutation).

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbHandle;
use crate::snippets::{self, CategoryAssign};
use crate::settings;

pub const KEY_ENABLED: &str = "sync.enabled";
pub const KEY_URL: &str = "sync.url";
pub const KEY_TOKEN: &str = "sync.token";
pub const KEY_LAST_MS: &str = "sync.last_ms";
pub const KEY_LAST_ERROR: &str = "sync.last_error";

pub const DEFAULT_URL: &str = "https://cue.celox.io";
const SYNC_INTERVAL_SECS: u64 = 60;
/// Debounce after a mutation-triggered wake so a burst of edits syncs once.
const WAKE_DEBOUNCE_MS: u64 = 1500;
const TOMBSTONE_TTL_MS: i64 = 90 * 24 * 3600 * 1000;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

// ── Config / status ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub enabled: bool,
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    /// Last successful cycle (ms epoch), 0 = never.
    pub last_ms: i64,
    /// Empty when the last cycle succeeded.
    pub last_error: String,
}

pub fn get_config(db: &DbHandle) -> anyhow::Result<SyncConfig> {
    Ok(SyncConfig {
        enabled: settings::get_bool(db, KEY_ENABLED, false)?,
        url: settings::get_or(db, KEY_URL, DEFAULT_URL)?,
        token: settings::get_or(db, KEY_TOKEN, "")?,
    })
}

pub fn set_config(db: &DbHandle, cfg: &SyncConfig) -> anyhow::Result<()> {
    settings::set(db, KEY_ENABLED, if cfg.enabled { "true" } else { "false" })?;
    let url = cfg.url.trim().trim_end_matches('/');
    settings::set(db, KEY_URL, if url.is_empty() { DEFAULT_URL } else { url })?;
    settings::set(db, KEY_TOKEN, cfg.token.trim())?;
    Ok(())
}

pub fn get_status(db: &DbHandle) -> anyhow::Result<SyncStatus> {
    Ok(SyncStatus {
        last_ms: settings::get_or(db, KEY_LAST_MS, "0")?.parse().unwrap_or(0),
        last_error: settings::get_or(db, KEY_LAST_ERROR, "")?,
    })
}

// ── Connection test (Settings-UI checkmarks) ─────────────────────────────────

/// Result of a live connection probe — drives the ✓/✗ chips in Settings →
/// Cloud-Sync. `reachable` = the server answered HTTP at all; `authorized` =
/// the token was accepted (only meaningful when `reachable`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncProbe {
    pub reachable: bool,
    pub authorized: bool,
    /// Human-readable detail for the failure chip ("" when all good).
    pub message: String,
}

/// Classify a probe outcome from the HTTP status (or `None` = network error).
/// Pure — unit-tested. 2xx → ok; 401/403 → server fine, token rejected;
/// other statuses → reachable but broken (surfaced verbatim).
pub fn classify_probe(status: Option<u16>, network_error: &str) -> SyncProbe {
    match status {
        None => SyncProbe {
            reachable: false,
            authorized: false,
            message: if network_error.is_empty() {
                "Server nicht erreichbar".to_string()
            } else {
                format!("Server nicht erreichbar: {network_error}")
            },
        },
        Some(s) if (200..300).contains(&s) => SyncProbe {
            reachable: true,
            authorized: true,
            message: String::new(),
        },
        Some(401) | Some(403) => SyncProbe {
            reachable: true,
            authorized: false,
            message: "Token ungültig oder abgelaufen".to_string(),
        },
        Some(s) => SyncProbe {
            reachable: true,
            authorized: false,
            message: format!("Server antwortet mit HTTP {s}"),
        },
    }
}

/// Live probe against the pull endpoint with the configured token. Blocking
/// (ureq) — call it from an async command / worker thread, never the main
/// thread. An empty token probes reachability only (expects 401 → reachable).
pub fn test_connection(cfg: &SyncConfig) -> SyncProbe {
    let url = format!("{}/api/sync/snippets", cfg.url.trim_end_matches('/'));
    let req = ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", cfg.token.trim()))
        .timeout(HTTP_TIMEOUT);
    match req.call() {
        Ok(resp) => classify_probe(Some(resp.status()), ""),
        // ureq returns HTTP-error statuses as Err(Status) — still "reachable".
        Err(ureq::Error::Status(code, _)) => classify_probe(Some(code), ""),
        Err(e) => classify_probe(None, &e.to_string()),
    }
}

// ── Wire format (matches cue's /api/sync/snippets) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSnippet {
    pub abbreviation: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// Group NAME; "" = ungrouped.
    #[serde(default)]
    pub category: String,
    #[serde(default = "one")]
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireTombstone {
    pub abbreviation: String,
    #[serde(default = "one")]
    pub version: i64,
    #[serde(default)]
    pub deleted_at_ms: i64,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Deserialize)]
pub struct PullResponse {
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub sync_ungrouped: bool,
    #[serde(default)]
    pub snippets: Vec<WireSnippet>,
    #[serde(default)]
    pub tombstones: Vec<WireTombstone>,
}

#[derive(Debug, Serialize)]
struct PushPayload {
    snippets: Vec<WireSnippet>,
    tombstones: Vec<WireTombstone>,
}

// ── Apply / collect (pure over the DB — unit-tested, no network) ─────────────

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ApplyStats {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub kept_local: usize,
}

impl ApplyStats {
    pub fn changed(&self) -> bool {
        self.created + self.updated + self.deleted > 0
    }
}

/// Apply a pull response to the local snippets table.
pub fn apply_pull(db: &DbHandle, pull: &PullResponse) -> anyhow::Result<ApplyStats> {
    let mut stats = ApplyStats::default();

    // Pre-create the scope groups so empty synced groups exist here too.
    for name in &pull.groups {
        let _ = snippets::category_id_for_name(db, name)?;
    }

    for ts in &pull.tombstones {
        match snippets::find_by_exact_abbreviation(db, &ts.abbreviation)? {
            Some(local) if local.version <= ts.version => {
                // snippets::delete records a local tombstone at local.version;
                // raise it to the remote version so the floor is right.
                snippets::delete(db, local.id)?;
                snippets::record_tombstone(db, &ts.abbreviation, ts.version)?;
                stats.deleted += 1;
            }
            Some(_) => stats.kept_local += 1, // local edit beat the deletion
            None => snippets::record_tombstone(db, &ts.abbreviation, ts.version)?,
        }
    }

    for item in &pull.snippets {
        if let Some(tsv) = snippets::tombstone_version(db, &item.abbreviation)? {
            if item.version <= tsv {
                continue; // deleted here — cue drops its copy via our push
            }
            snippets::clear_tombstone(db, &item.abbreviation)?;
        }
        let assign = if item.category.is_empty() {
            CategoryAssign::Clear
        } else {
            match snippets::category_id_for_name(db, &item.category)? {
                Some(id) => CategoryAssign::Set(id),
                None => CategoryAssign::Clear,
            }
        };
        match snippets::find_by_exact_abbreviation(db, &item.abbreviation)? {
            None => {
                snippets::upsert_by_abbreviation(
                    db,
                    &item.abbreviation,
                    &item.title,
                    &item.body,
                    assign,
                    item.version,
                )?;
                stats.created += 1;
            }
            Some(local) if item.version > local.version => {
                snippets::upsert_by_abbreviation(
                    db,
                    &item.abbreviation,
                    &item.title,
                    &item.body,
                    assign,
                    item.version,
                )?;
                stats.updated += 1;
            }
            Some(local) if item.version == local.version => {
                let identical = local.title == item.title && local.body == item.body;
                if identical {
                    // cue is the organizational master: adopt its grouping.
                    let local_cat = local.category.clone().unwrap_or_default();
                    if local_cat != item.category {
                        snippets::set_category(db, local.id, assign.value())?;
                        stats.updated += 1;
                    }
                } else {
                    // Equal-version conflict: cue wins. The merge bumps to
                    // local+1; the next push converges cue onto that version.
                    snippets::upsert_by_abbreviation(
                        db,
                        &item.abbreviation,
                        &item.title,
                        &item.body,
                        assign,
                        item.version,
                    )?;
                    stats.updated += 1;
                }
            }
            Some(_) => stats.kept_local += 1, // local is newer — push carries it
        }
    }
    Ok(stats)
}

/// Everything this side sends back: scope snippets + all local tombstones.
pub fn build_push(db: &DbHandle, pull: &PullResponse) -> anyhow::Result<(Vec<WireSnippet>, Vec<WireTombstone>)> {
    let groups: HashSet<&str> = pull.groups.iter().map(String::as_str).collect();
    let snippets_out = snippets::list_all(db)?
        .into_iter()
        .filter(|s| match &s.category {
            Some(name) => groups.contains(name.as_str()),
            None => pull.sync_ungrouped,
        })
        .map(|s| WireSnippet {
            abbreviation: s.abbreviation,
            title: s.title,
            body: s.body,
            category: s.category.unwrap_or_default(),
            version: s.version,
        })
        .collect();
    let tombstones_out = snippets::list_tombstones(db)?
        .into_iter()
        .map(|(abbreviation, version, deleted_at_ms)| WireTombstone {
            abbreviation,
            version,
            deleted_at_ms,
        })
        .collect();
    Ok((snippets_out, tombstones_out))
}

// ── Network cycle ────────────────────────────────────────────────────────────

fn cycle(db: &DbHandle, cfg: &SyncConfig) -> Result<ApplyStats, String> {
    let base = cfg.url.trim_end_matches('/');
    let url = format!("{base}/api/sync/snippets");
    let auth = format!("Bearer {}", cfg.token);

    let resp = ureq::get(&url)
        .set("Authorization", &auth)
        .timeout(HTTP_TIMEOUT)
        .call()
        .map_err(|e| format!("Pull fehlgeschlagen: {e}"))?;
    // Bounded read + manual parse (ureq is built without the json feature).
    let mut body = String::new();
    use std::io::Read;
    resp.into_reader()
        .take(32 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("Antwort unlesbar: {e}"))?;
    let pull: PullResponse =
        serde_json::from_str(&body).map_err(|e| format!("Antwort unlesbar: {e}"))?;

    let stats = apply_pull(db, &pull).map_err(|e| format!("Anwenden fehlgeschlagen: {e}"))?;

    let (snippets_out, tombstones_out) =
        build_push(db, &pull).map_err(|e| format!("Sammeln fehlgeschlagen: {e}"))?;
    let payload = PushPayload {
        snippets: snippets_out,
        tombstones: tombstones_out,
    };
    ureq::post(&url)
        .set("Authorization", &auth)
        .set("Content-Type", "application/json")
        .timeout(HTTP_TIMEOUT)
        .send_string(&serde_json::to_string(&payload).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Push fehlgeschlagen: {e}"))?;

    let _ = snippets::prune_tombstones(db, TOMBSTONE_TTL_MS);
    Ok(stats)
}

// ── Worker thread ────────────────────────────────────────────────────────────

static WAKER: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

/// Ask the worker to sync soon (debounced). Safe to call from anywhere; a
/// no-op until [`start`] ran (and when sync is disabled — the worker checks).
pub fn request_sync() {
    if let Some(tx) = WAKER.get() {
        let _ = tx.lock().map(|tx| tx.send(()));
    }
}

pub fn start(app: AppHandle, db: DbHandle) {
    let (tx, rx): (Sender<()>, Receiver<()>) = std::sync::mpsc::channel();
    let _ = WAKER.set(Mutex::new(tx));
    std::thread::Builder::new()
        .name("ir-cue-sync".into())
        .spawn(move || loop {
            // Wait for the interval tick or an explicit wake; then debounce so
            // a burst of edits results in one cycle.
            let woken = rx.recv_timeout(Duration::from_secs(SYNC_INTERVAL_SECS)).is_ok();
            if woken {
                std::thread::sleep(Duration::from_millis(WAKE_DEBOUNCE_MS));
                while rx.try_recv().is_ok() {}
            }
            let cfg = match get_config(&db) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if !cfg.enabled || cfg.token.is_empty() {
                continue;
            }
            match cycle(&db, &cfg) {
                Ok(stats) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let _ = settings::set(&db, KEY_LAST_MS, &now.to_string());
                    let _ = settings::set(&db, KEY_LAST_ERROR, "");
                    if stats.changed() {
                        // Rebuild the expander table + tell the UI to reload.
                        let ae = app.state::<crate::auto_expand::AutoExpandState>();
                        crate::auto_expand::rebuild_table(&db, &ae);
                        let _ = app.emit("snippets-synced", ());
                    }
                }
                Err(err) => {
                    let _ = settings::set(&db, KEY_LAST_ERROR, &err);
                }
            }
            // Tell the Settings UI a cycle finished (success or failure) so the
            // status chips refresh live instead of blind-polling.
            let _ = app.emit("sync-status-changed", ());
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex as PlMutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().expect("mem db");
        let db: DbHandle = Arc::new(PlMutex::new(conn));
        snippets::init_table(&db).expect("snippets schema");
        settings::init_table(&db).expect("settings schema");
        db
    }

    #[test]
    fn classify_probe_maps_the_outcome_matrix() {
        // 2xx → fully ok, no message.
        assert_eq!(
            classify_probe(Some(200), ""),
            SyncProbe { reachable: true, authorized: true, message: String::new() }
        );
        assert!(classify_probe(Some(204), "").authorized);
        // 401/403 → server fine, token rejected.
        for s in [401u16, 403] {
            let p = classify_probe(Some(s), "");
            assert!(p.reachable && !p.authorized, "status {s}");
            assert!(p.message.contains("Token"), "status {s}: {}", p.message);
        }
        // Other HTTP errors → reachable, surfaced verbatim.
        let p = classify_probe(Some(500), "");
        assert!(p.reachable && !p.authorized);
        assert!(p.message.contains("500"));
        // Network error → unreachable, detail carried through.
        let p = classify_probe(None, "dns lookup failed");
        assert!(!p.reachable && !p.authorized);
        assert!(p.message.contains("dns lookup failed"));
        // Network error without detail still explains itself.
        assert!(!classify_probe(None, "").message.is_empty());
    }

    fn wire(abbr: &str, body: &str, category: &str, version: i64) -> WireSnippet {
        WireSnippet {
            abbreviation: abbr.into(),
            title: String::new(),
            body: body.into(),
            category: category.into(),
            version,
        }
    }

    fn pull(
        groups: &[&str],
        ungrouped: bool,
        snips: Vec<WireSnippet>,
        tombs: Vec<WireTombstone>,
    ) -> PullResponse {
        PullResponse {
            groups: groups.iter().map(|s| s.to_string()).collect(),
            sync_ungrouped: ungrouped,
            snippets: snips,
            tombstones: tombs,
        }
    }

    fn body_of(db: &DbHandle, abbr: &str) -> Option<(String, i64)> {
        snippets::find_by_exact_abbreviation(db, abbr)
            .unwrap()
            .map(|s| (s.body, s.version))
    }

    #[test]
    fn apply_creates_updates_and_keeps_by_version() {
        let db = test_db();
        // New from cue → created, version adopted, group created + assigned.
        let stats = apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "v3", "AI", 3)], vec![]))
            .unwrap();
        assert_eq!(stats.created, 1);
        let s = snippets::find_by_exact_abbreviation(&db, "a").unwrap().unwrap();
        assert_eq!((s.body.as_str(), s.version, s.category.as_deref()), ("v3", 3, Some("AI")));

        // Higher incoming wins.
        let stats = apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "v5", "AI", 5)], vec![]))
            .unwrap();
        assert_eq!(stats.updated, 1);
        assert_eq!(body_of(&db, "a"), Some(("v5".into(), 5)));

        // Lower incoming is ignored.
        let stats = apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "OLD", "AI", 4)], vec![]))
            .unwrap();
        assert_eq!(stats.kept_local, 1);
        assert_eq!(body_of(&db, "a"), Some(("v5".into(), 5)));

        // Equal version + identical content → pure no-op.
        let stats = apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "v5", "AI", 5)], vec![]))
            .unwrap();
        assert_eq!(stats, ApplyStats::default());
    }

    #[test]
    fn equal_version_conflict_cue_wins_with_bump() {
        let db = test_db();
        apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "local", "AI", 2)], vec![])).unwrap();
        // Remote has DIFFERENT content at the SAME version → adopt + bump to 3.
        let stats =
            apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "cue", "AI", 2)], vec![])).unwrap();
        assert_eq!(stats.updated, 1);
        assert_eq!(body_of(&db, "a"), Some(("cue".into(), 3)));
        // Replaying cue's (now-lower) copy changes nothing further.
        let stats =
            apply_pull(&db, &pull(&["AI"], false, vec![wire("a", "cue", "AI", 2)], vec![])).unwrap();
        assert_eq!(stats.kept_local, 1);
    }

    #[test]
    fn tie_adopts_cue_grouping_without_version_bump() {
        let db = test_db();
        apply_pull(&db, &pull(&["AI", "Zwei"], false, vec![wire("a", "b", "AI", 1)], vec![]))
            .unwrap();
        let stats =
            apply_pull(&db, &pull(&["AI", "Zwei"], false, vec![wire("a", "b", "Zwei", 1)], vec![]))
                .unwrap();
        assert_eq!(stats.updated, 1);
        let s = snippets::find_by_exact_abbreviation(&db, "a").unwrap().unwrap();
        assert_eq!((s.category.as_deref(), s.version), (Some("Zwei"), 1));
    }

    #[test]
    fn tombstone_deletes_unless_local_is_newer() {
        let db = test_db();
        snippets::create(&db, "dead", "", "b", None).unwrap(); // v1
        snippets::create(&db, "alive", "", "b", None).unwrap(); // v1
        let id = snippets::find_by_exact_abbreviation(&db, "alive").unwrap().unwrap().id;
        snippets::update(&db, id, "alive", "", "edited", None).unwrap(); // v2

        let tomb = |abbr: &str| WireTombstone {
            abbreviation: abbr.into(),
            version: 1,
            deleted_at_ms: 0,
        };
        let stats =
            apply_pull(&db, &pull(&[], true, vec![], vec![tomb("dead"), tomb("alive")])).unwrap();
        assert_eq!((stats.deleted, stats.kept_local), (1, 1));
        assert!(body_of(&db, "dead").is_none());
        assert_eq!(body_of(&db, "alive"), Some(("edited".into(), 2)));

        // A remote snippet at/below the stored tombstone stays dead …
        let stats =
            apply_pull(&db, &pull(&[], true, vec![wire("dead", "x", "", 1)], vec![])).unwrap();
        assert!(!stats.changed());
        assert!(body_of(&db, "dead").is_none());
        // … but a higher-versioned one resurrects (and clears the tombstone).
        let stats =
            apply_pull(&db, &pull(&[], true, vec![wire("dead", "x", "", 2)], vec![])).unwrap();
        assert_eq!(stats.created, 1);
        assert_eq!(snippets::tombstone_version(&db, "dead").unwrap(), None);
    }

    #[test]
    fn local_delete_and_recreate_land_above_the_tombstone() {
        let db = test_db();
        snippets::create(&db, "gone", "", "b", None).unwrap();
        let id = snippets::find_by_exact_abbreviation(&db, "gone").unwrap().unwrap().id;
        snippets::update(&db, id, "gone", "", "b2", None).unwrap(); // v2
        snippets::delete(&db, id).unwrap();
        assert_eq!(snippets::tombstone_version(&db, "gone").unwrap(), Some(2));
        // Recreate → v3, tombstone cleared.
        snippets::create(&db, "gone", "", "new", None).unwrap();
        assert_eq!(body_of(&db, "gone"), Some(("new".into(), 3)));
        assert_eq!(snippets::tombstone_version(&db, "gone").unwrap(), None);
    }

    #[test]
    fn rename_tombstones_the_old_abbreviation() {
        let db = test_db();
        snippets::create(&db, "old", "", "b", None).unwrap();
        let id = snippets::find_by_exact_abbreviation(&db, "old").unwrap().unwrap().id;
        snippets::update(&db, id, "new", "", "b", None).unwrap();
        assert_eq!(snippets::tombstone_version(&db, "old").unwrap(), Some(1));
        // Renaming back resurrects above the old tombstone.
        snippets::update(&db, id, "old", "", "b", None).unwrap();
        assert_eq!(snippets::tombstone_version(&db, "old").unwrap(), None);
        let s = snippets::find_by_exact_abbreviation(&db, "old").unwrap().unwrap();
        assert!(s.version >= 2);
        assert_eq!(snippets::tombstone_version(&db, "new").unwrap(), Some(2));
    }

    #[test]
    fn build_push_respects_the_scope() {
        let db = test_db();
        let ai = snippets::create_category(&db, "AI").unwrap();
        let secret = snippets::create_category(&db, "Privat").unwrap();
        snippets::create(&db, "a", "", "b", Some(ai)).unwrap();
        snippets::create(&db, "p", "", "b", Some(secret)).unwrap();
        snippets::create(&db, "u", "", "b", None).unwrap();
        snippets::create(&db, "dead", "", "b", None).unwrap();
        let id = snippets::find_by_exact_abbreviation(&db, "dead").unwrap().unwrap().id;
        snippets::delete(&db, id).unwrap();

        let scope = pull(&["AI"], false, vec![], vec![]);
        let (snips, tombs) = build_push(&db, &scope).unwrap();
        assert_eq!(
            snips.iter().map(|s| s.abbreviation.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(tombs.len(), 1);
        assert_eq!(tombs[0].abbreviation, "dead");

        let scope = pull(&["AI"], true, vec![], vec![]);
        let (snips, _) = build_push(&db, &scope).unwrap();
        let mut names: Vec<_> = snips.iter().map(|s| s.abbreviation.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["a", "u"]);
    }

    #[test]
    fn two_cycle_convergence_is_a_noop() {
        let db = test_db();
        // Cycle 1: cue sends one snippet; we have one of our own in scope.
        let ai = snippets::create_category(&db, "AI").unwrap();
        snippets::create(&db, "mine", "", "local", Some(ai)).unwrap();
        let remote = pull(&["AI"], false, vec![wire("cue", "remote", "AI", 1)], vec![]);
        let stats = apply_pull(&db, &remote).unwrap();
        assert_eq!(stats.created, 1);
        // Cycle 2 with the SAME state (as if cue accepted our push) → no-op.
        let remote2 = pull(
            &["AI"],
            false,
            vec![wire("cue", "remote", "AI", 1), wire("mine", "local", "AI", 1)],
            vec![],
        );
        let stats = apply_pull(&db, &remote2).unwrap();
        assert_eq!(stats, ApplyStats::default());
    }

    /// Live e2e against a locally running cue backend (real HTTP + wire
    /// format). Ignored by default — run with:
    /// `CUE_SYNC_URL=http://127.0.0.1:8123 CUE_SYNC_TOKEN=… cargo test -p
    ///  inspector-rust-core --lib sync -- --ignored`
    #[test]
    #[ignore]
    fn live_cycle_against_local_cue() {
        let url = std::env::var("CUE_SYNC_URL").expect("CUE_SYNC_URL not set");
        let token = std::env::var("CUE_SYNC_TOKEN").expect("CUE_SYNC_TOKEN not set");
        let db = test_db();
        let ai = snippets::create_category(&db, "AI Prompts").unwrap();
        snippets::create(&db, "irlocal", "", "from ir", Some(ai)).unwrap();

        let cfg = SyncConfig { enabled: true, url, token };
        let stats = cycle(&db, &cfg).expect("cycle 1");
        // cue's seeded snippet arrived here, in the right group.
        let got = snippets::find_by_exact_abbreviation(&db, "cueseed")
            .unwrap()
            .unwrap_or_else(|| panic!("cueseed missing after cycle 1 ({stats:?})"));
        assert_eq!((got.body.as_str(), got.category.as_deref()), ("from cue", Some("AI Prompts")));
        // The second cycle is a complete no-op — converged, no ping-pong.
        let stats2 = cycle(&db, &cfg).expect("cycle 2");
        assert!(!stats2.changed(), "second cycle should be a no-op: {stats2:?}");
    }

    #[test]
    fn config_roundtrip_and_url_normalisation() {
        let db = test_db();
        let cfg = SyncConfig {
            enabled: true,
            url: "https://cue.celox.io/".into(),
            token: "  tok  ".into(),
        };
        set_config(&db, &cfg).unwrap();
        let got = get_config(&db).unwrap();
        assert!(got.enabled);
        assert_eq!(got.url, "https://cue.celox.io");
        assert_eq!(got.token, "tok");
        // Defaults when nothing is stored.
        let fresh = test_db();
        let got = get_config(&fresh).unwrap();
        assert_eq!((got.enabled, got.url.as_str(), got.token.as_str()), (false, DEFAULT_URL, ""));
    }
}
