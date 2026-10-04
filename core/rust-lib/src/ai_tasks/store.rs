//! Tasks and their runs in the app database. Prompt, script, explanation and
//! run output are encrypted at rest like clipboard content.
//!
//! Approval is a hash, not a flag: a task is approved exactly when the hash
//! the user approved equals the hash of the CURRENT language + script. Any
//! change to either withdraws it — there is no code path that edits a script
//! and keeps the approval.

use super::generate::Language;
use super::provider::ProviderId;
use super::runner::{RunOutcome, DEFAULT_TIMEOUT_S, MAX_TIMEOUT_S};
use super::trigger::Trigger;
use crate::crypto::{decrypt, encrypt};
use crate::db::DbHandle;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Runs kept per task; older ones are pruned on insert.
pub const RUNS_KEPT: i64 = 50;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ai_tasks (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL,
            prompt        TEXT NOT NULL,
            language      TEXT NOT NULL,
            script        TEXT NOT NULL,
            explanation   TEXT NOT NULL DEFAULT '',
            provider      TEXT NOT NULL,
            trigger       TEXT NOT NULL,
            enabled       INTEGER NOT NULL DEFAULT 1,
            timeout_s     INTEGER NOT NULL DEFAULT 120,
            approved_hash TEXT,
            created_ms    INTEGER NOT NULL,
            updated_ms    INTEGER NOT NULL,
            last_fire_ms  INTEGER
        );
        CREATE TABLE IF NOT EXISTS ai_task_runs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id     INTEGER NOT NULL,
            started_ms  INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            exit_code   INTEGER,
            timed_out   INTEGER NOT NULL DEFAULT 0,
            trigger     TEXT NOT NULL,
            output      TEXT NOT NULL DEFAULT '',
            error       TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_ai_task_runs_task ON ai_task_runs(task_id, started_ms DESC);",
    )
}

/// The approval fingerprint of a script version.
pub fn script_hash(language: Language, script: &str) -> String {
    let mut h = Sha256::new();
    h.update(language.as_str().as_bytes());
    h.update(b"\x00");
    h.update(script.as_bytes());
    format!("{:x}", h.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: i64,
    pub name: String,
    /// The description the script was generated from.
    pub prompt: String,
    pub language: Language,
    pub script: String,
    pub explanation: String,
    pub provider: ProviderId,
    pub trigger: Trigger,
    pub enabled: bool,
    pub timeout_s: u32,
    /// Hash of the current version — what an approval must name.
    pub script_hash: String,
    pub approved: bool,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub last_fire_ms: Option<i64>,
    /// The most recent run, if any.
    pub last_run: Option<Run>,
}

/// What the editor sends: everything the user can change.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskDraft {
    pub id: Option<i64>,
    pub name: String,
    pub prompt: String,
    pub language: Language,
    pub script: String,
    #[serde(default)]
    pub explanation: String,
    pub provider: ProviderId,
    pub trigger: Trigger,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_s: u32,
}

fn yes() -> bool {
    true
}
fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_S
}

pub fn clamp_timeout(s: u32) -> u32 {
    s.clamp(5, MAX_TIMEOUT_S)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Run {
    pub id: i64,
    pub task_id: i64,
    pub started_ms: i64,
    pub duration_ms: i64,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub trigger: String,
    pub output: String,
    /// Set when the script could not even be started.
    pub error: Option<String>,
}

impl Run {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn ok(&self) -> bool {
        self.error.is_none() && !self.timed_out && self.exit_code == Some(0)
    }
}

const TASK_COLS: &str = "id, name, prompt, language, script, explanation, provider, trigger, enabled, \
                         timeout_s, approved_hash, created_ms, updated_ms, last_fire_ms";

fn row_to_task(r: &Row<'_>) -> rusqlite::Result<(Task, Option<String>)> {
    let lang: String = r.get(3)?;
    let provider: String = r.get(6)?;
    let trigger: String = r.get(7)?;
    let language = Language::parse(&lang).unwrap_or(Language::Bash);
    let script = decrypt(&r.get::<_, String>(4)?);
    let hash = script_hash(language, &script);
    let approved_hash: Option<String> = r.get(10)?;
    Ok((
        Task {
            id: r.get(0)?,
            name: r.get(1)?,
            prompt: decrypt(&r.get::<_, String>(2)?),
            language,
            approved: approved_hash.as_deref() == Some(hash.as_str()),
            script_hash: hash,
            script,
            explanation: decrypt(&r.get::<_, String>(5)?),
            provider: ProviderId::parse(&provider).unwrap_or(ProviderId::Anthropic),
            trigger: serde_json::from_str(&trigger).unwrap_or(Trigger::Manual),
            enabled: r.get::<_, i64>(8)? != 0,
            timeout_s: r.get::<_, i64>(9)?.clamp(5, i64::from(MAX_TIMEOUT_S)) as u32,
            created_ms: r.get(11)?,
            updated_ms: r.get(12)?,
            last_fire_ms: r.get(13)?,
            last_run: None,
        },
        approved_hash,
    ))
}

fn row_to_run(r: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        task_id: r.get(1)?,
        started_ms: r.get(2)?,
        duration_ms: r.get(3)?,
        exit_code: r.get(4)?,
        timed_out: r.get::<_, i64>(5)? != 0,
        trigger: r.get(6)?,
        output: decrypt(&r.get::<_, String>(7)?),
        error: r.get(8)?,
    })
}

const RUN_COLS: &str = "id, task_id, started_ms, duration_ms, exit_code, timed_out, trigger, output, error";

fn last_run(conn: &Connection, task_id: i64) -> Option<Run> {
    conn.query_row(
        &format!("SELECT {RUN_COLS} FROM ai_task_runs WHERE task_id = ?1 ORDER BY started_ms DESC, id DESC LIMIT 1"),
        params![task_id],
        row_to_run,
    )
    .optional()
    .ok()
    .flatten()
}

pub fn list(db: &DbHandle) -> Result<Vec<Task>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(&format!("SELECT {TASK_COLS} FROM ai_tasks ORDER BY name COLLATE NOCASE, id"))?;
    let rows = stmt.query_map([], row_to_task)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .map(|(mut t, _)| {
            t.last_run = last_run(&conn, t.id);
            t
        })
        .collect())
}

pub fn get(db: &DbHandle, id: i64) -> Result<Option<Task>> {
    let conn = db.lock();
    let found = conn
        .query_row(&format!("SELECT {TASK_COLS} FROM ai_tasks WHERE id = ?1"), params![id], row_to_task)
        .optional()?;
    Ok(found.map(|(mut t, _)| {
        t.last_run = last_run(&conn, t.id);
        t
    }))
}

/// Create or update. A changed language/script loses its approval by
/// construction (the stored hash no longer matches); a save that leaves them
/// unchanged keeps it. Returns the saved task.
pub fn save(db: &DbHandle, d: &TaskDraft, now_ms: i64) -> Result<Task> {
    let name: String = d.name.trim().chars().take(super::generate::MAX_NAME_CHARS).collect();
    if name.is_empty() {
        return Err(anyhow!("Der Task braucht einen Namen."));
    }
    if d.script.trim().is_empty() {
        return Err(anyhow!("Das Skript ist leer."));
    }
    if d.script.len() > super::generate::MAX_SCRIPT_BYTES {
        return Err(anyhow!("Das Skript ist zu lang (über 64 KB)."));
    }
    let trigger = d.trigger.clone().normalized();
    if let Trigger::Folder { path } = &trigger {
        if !std::path::Path::new(path).is_absolute() {
            return Err(anyhow!("Der überwachte Ordner braucht einen vollständigen Pfad."));
        }
    }
    let trig_json = serde_json::to_string(&trigger)?;
    let id = {
        let conn = db.lock();
        match d.id {
            Some(id) => {
                let n = conn.execute(
                    "UPDATE ai_tasks SET name=?1, prompt=?2, language=?3, script=?4, explanation=?5,
                         provider=?6, trigger=?7, enabled=?8, timeout_s=?9, updated_ms=?10
                     WHERE id=?11",
                    params![
                        name,
                        encrypt(&d.prompt),
                        d.language.as_str(),
                        encrypt(&d.script),
                        encrypt(&d.explanation),
                        d.provider.as_str(),
                        trig_json,
                        d.enabled as i64,
                        clamp_timeout(d.timeout_s),
                        now_ms,
                        id
                    ],
                )?;
                if n == 0 {
                    return Err(anyhow!("Task {id} gibt es nicht mehr."));
                }
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO ai_tasks (name, prompt, language, script, explanation, provider, trigger,
                         enabled, timeout_s, approved_hash, created_ms, updated_ms)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10,?10)",
                    params![
                        name,
                        encrypt(&d.prompt),
                        d.language.as_str(),
                        encrypt(&d.script),
                        encrypt(&d.explanation),
                        d.provider.as_str(),
                        trig_json,
                        d.enabled as i64,
                        clamp_timeout(d.timeout_s),
                        now_ms
                    ],
                )?;
                conn.last_insert_rowid()
            }
        }
    };
    get(db, id)?.context("saved task vanished")
}

/// Approve exactly the version the user looked at. A hash that no longer
/// matches (the script changed meanwhile) is refused, never silently applied.
pub fn approve(db: &DbHandle, id: i64, hash: &str, now_ms: i64) -> Result<Task> {
    let task = get(db, id)?.ok_or_else(|| anyhow!("Task {id} gibt es nicht mehr."))?;
    if task.script_hash != hash {
        return Err(anyhow!("Das Skript hat sich geändert — bitte die aktuelle Fassung ansehen und erneut freigeben."));
    }
    {
        let conn = db.lock();
        // Interval tasks count from the approval, not from creation.
        conn.execute(
            "UPDATE ai_tasks SET approved_hash=?1, last_fire_ms=?2 WHERE id=?3",
            params![hash, now_ms, id],
        )?;
    }
    get(db, id)?.context("approved task vanished")
}

pub fn revoke(db: &DbHandle, id: i64) -> Result<()> {
    db.lock().execute("UPDATE ai_tasks SET approved_hash=NULL WHERE id=?1", params![id])?;
    Ok(())
}

pub fn set_enabled(db: &DbHandle, id: i64, enabled: bool, now_ms: i64) -> Result<()> {
    // Re-enabling restarts an interval from now instead of firing a backlog.
    db.lock().execute(
        "UPDATE ai_tasks SET enabled=?1, last_fire_ms=CASE WHEN ?1=1 THEN ?2 ELSE last_fire_ms END WHERE id=?3",
        params![enabled as i64, now_ms, id],
    )?;
    Ok(())
}

pub fn set_last_fire(db: &DbHandle, id: i64, ms: i64) -> Result<()> {
    db.lock().execute("UPDATE ai_tasks SET last_fire_ms=?1 WHERE id=?2", params![ms, id])?;
    Ok(())
}

pub fn delete(db: &DbHandle, id: i64) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM ai_task_runs WHERE task_id=?1", params![id])?;
    conn.execute("DELETE FROM ai_tasks WHERE id=?1", params![id])?;
    Ok(())
}

/// Record a run (or a failure to start) and prune old ones.
pub fn record_run(
    db: &DbHandle,
    task_id: i64,
    started_ms: i64,
    trigger: &str,
    result: &Result<RunOutcome, String>,
) -> Result<Run> {
    let conn = db.lock();
    match result {
        Ok(o) => conn.execute(
            "INSERT INTO ai_task_runs (task_id, started_ms, duration_ms, exit_code, timed_out, trigger, output, error)
             VALUES (?1,?2,?3,?4,?5,?6,?7,NULL)",
            params![task_id, started_ms, o.duration_ms, o.exit_code, o.timed_out as i64, trigger, encrypt(&o.output)],
        )?,
        Err(e) => conn.execute(
            "INSERT INTO ai_task_runs (task_id, started_ms, duration_ms, exit_code, timed_out, trigger, output, error)
             VALUES (?1,?2,0,NULL,0,?3,'',?4)",
            params![task_id, started_ms, trigger, e],
        )?,
    };
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM ai_task_runs WHERE task_id=?1 AND id NOT IN
           (SELECT id FROM ai_task_runs WHERE task_id=?1 ORDER BY started_ms DESC, id DESC LIMIT ?2)",
        params![task_id, RUNS_KEPT],
    )?;
    let run = conn.query_row(&format!("SELECT {RUN_COLS} FROM ai_task_runs WHERE id=?1"), params![id], row_to_run)?;
    Ok(run)
}

pub fn runs(db: &DbHandle, task_id: i64, limit: i64) -> Result<Vec<Run>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(&format!(
        "SELECT {RUN_COLS} FROM ai_task_runs WHERE task_id=?1 ORDER BY started_ms DESC, id DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![task_id, limit.clamp(1, RUNS_KEPT)], row_to_run)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn db() -> DbHandle {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn draft(script: &str) -> TaskDraft {
        TaskDraft {
            id: None,
            name: "Aufräumen".into(),
            prompt: "Räume auf".into(),
            language: Language::Zsh,
            script: script.into(),
            explanation: "erklärt".into(),
            provider: ProviderId::Gemini,
            trigger: Trigger::Interval { minutes: 30 },
            enabled: true,
            timeout_s: 60,
        }
    }

    #[test]
    fn a_new_task_is_never_approved() {
        let db = db();
        let t = save(&db, &draft("echo 1"), 1).unwrap();
        assert!(!t.approved);
        assert_eq!(t.provider, ProviderId::Gemini);
        assert_eq!(t.trigger, Trigger::Interval { minutes: 30 });
    }

    #[test]
    fn approval_names_the_version_and_any_script_change_withdraws_it() {
        let db = db();
        let t = save(&db, &draft("echo 1"), 1).unwrap();
        let t = approve(&db, t.id, &t.script_hash, 2).unwrap();
        assert!(t.approved);
        // Saving other fields keeps it.
        let mut d = draft("echo 1");
        d.id = Some(t.id);
        d.name = "Neu".into();
        d.trigger = Trigger::Wake;
        assert!(save(&db, &d, 3).unwrap().approved);
        // Changing the script withdraws it…
        d.script = "echo 2".into();
        let changed = save(&db, &d, 4).unwrap();
        assert!(!changed.approved);
        // …and so does changing only the language.
        let mut back = d.clone();
        back.script = "echo 1".into();
        back.language = Language::Bash;
        assert!(!save(&db, &back, 5).unwrap().approved);
    }

    #[test]
    fn an_outdated_approval_is_refused() {
        let db = db();
        let t = save(&db, &draft("echo 1"), 1).unwrap();
        let seen = t.script_hash.clone();
        let mut d = draft("rm -rf ~/x");
        d.id = Some(t.id);
        save(&db, &d, 2).unwrap();
        assert!(approve(&db, t.id, &seen, 3).is_err(), "approving what you saw must not approve what's there now");
        assert!(!get(&db, t.id).unwrap().unwrap().approved);
    }

    #[test]
    fn the_script_is_stored_encrypted_column_and_reads_back() {
        let db = db();
        let t = save(&db, &draft("echo geheim"), 1).unwrap();
        assert_eq!(get(&db, t.id).unwrap().unwrap().script, "echo geheim");
    }

    #[test]
    fn invalid_drafts_are_rejected() {
        let db = db();
        let mut d = draft("echo");
        d.name = "  ".into();
        assert!(save(&db, &d, 1).is_err());
        let mut d = draft("   ");
        d.name = "x".into();
        assert!(save(&db, &d, 1).is_err());
        let mut d = draft("echo");
        d.trigger = Trigger::Folder { path: "Downloads".into() };
        assert!(save(&db, &d, 1).is_err(), "a relative folder is ambiguous for a background watcher");
        let mut d = draft("echo");
        d.id = Some(999);
        assert!(save(&db, &d, 1).is_err());
    }

    #[test]
    fn timeouts_and_triggers_are_normalised_on_save() {
        let db = db();
        let mut d = draft("echo");
        d.timeout_s = 0;
        d.trigger = Trigger::Interval { minutes: 0 };
        let t = save(&db, &d, 1).unwrap();
        assert_eq!(t.timeout_s, 5);
        assert_eq!(t.trigger, Trigger::Interval { minutes: 1 });
    }

    #[test]
    fn runs_are_recorded_newest_first_and_pruned() {
        let db = db();
        let t = save(&db, &draft("echo"), 1).unwrap();
        for i in 0..(RUNS_KEPT + 5) {
            let ok: Result<RunOutcome, String> =
                Ok(RunOutcome { exit_code: Some(0), timed_out: false, duration_ms: 3, output: format!("run {i}") });
            record_run(&db, t.id, 100 + i, "interval", &ok).unwrap();
        }
        let r = runs(&db, t.id, 100).unwrap();
        assert_eq!(r.len() as i64, RUNS_KEPT);
        assert_eq!(r[0].output, format!("run {}", RUNS_KEPT + 4));
        let failed = record_run(&db, t.id, 999, "manual", &Err("kein python".into())).unwrap();
        assert!(!failed.ok());
        assert_eq!(get(&db, t.id).unwrap().unwrap().last_run.unwrap().error.as_deref(), Some("kein python"));
    }

    #[test]
    fn deleting_a_task_removes_its_runs() {
        let db = db();
        let t = save(&db, &draft("echo"), 1).unwrap();
        record_run(&db, t.id, 1, "manual", &Err("x".into())).unwrap();
        delete(&db, t.id).unwrap();
        assert!(get(&db, t.id).unwrap().is_none());
        assert!(runs(&db, t.id, 10).unwrap().is_empty());
    }

    #[test]
    fn re_enabling_restarts_the_interval_clock() {
        let db = db();
        let t = save(&db, &draft("echo"), 1).unwrap();
        set_enabled(&db, t.id, false, 50).unwrap();
        assert_eq!(get(&db, t.id).unwrap().unwrap().last_fire_ms, None);
        set_enabled(&db, t.id, true, 77).unwrap();
        assert_eq!(get(&db, t.id).unwrap().unwrap().last_fire_ms, Some(77));
    }
}
