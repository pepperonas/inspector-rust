//! The background thread that starts approved tasks.
//!
//! One loop owns all decisions (timed tasks, events, folder batches); runs
//! happen on their own threads so a slow script never delays another task.
//! The decisions are pure functions below and unit-tested; the loop only
//! wires them to the clock, the database and the file watcher.

use super::store::{self, Task};
use super::trigger::{Event, Trigger};
use super::{is_paused, runner};
use crate::db::DbHandle;
use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};

/// How often the loop looks at the clock without a message.
const TICK: Duration = Duration::from_secs(5);
/// A folder batch fires this long after its last change — a download or a
/// copy is many events, the task should run once, on the finished file.
pub const FOLDER_QUIET_MS: i64 = 2_000;
/// Event-triggered tasks run at most this often, and a task's own writes into
/// its watched folder (during the run and shortly after) don't restart it.
pub const EVENT_MIN_GAP_MS: i64 = 10_000;
/// A wall-clock jump this much larger than monotonic time means the computer slept.
const SLEPT_SLACK: Duration = Duration::from_secs(60);
/// Paths passed to a folder task per run.
pub const MAX_PATHS: usize = 200;

pub enum Msg {
    /// Tasks changed — re-read them and the folder watchers.
    Reload,
    Event(Event),
    FolderChange { folder: String, path: String },
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

fn send(m: Msg) {
    if let Some(tx) = TX.get() {
        let _ = tx.lock().send(m);
    }
}

/// After any task change (save, approve, enable, delete, pause).
pub fn reload() {
    send(Msg::Reload);
}

// ── Pure decisions ───────────────────────────────────────────────────────────

/// May this task start automatically at all?
pub fn eligible(t: &Task, paused: bool) -> bool {
    !paused && t.enabled && t.approved && t.trigger != Trigger::Manual
}

/// Timed tasks due at `now`. A task never fired before counts from `now`
/// (its first run is one interval away, not immediately).
pub fn due_now<Tz: chrono::TimeZone>(tasks: &[Task], paused: bool, now_ms: i64, tz: &Tz) -> Vec<i64> {
    tasks
        .iter()
        .filter(|t| eligible(t, paused))
        .filter(|t| {
            let base = t.last_fire_ms.unwrap_or(now_ms);
            t.trigger.next_after(base, tz).is_some_and(|due| due <= now_ms)
        })
        .map(|t| t.id)
        .collect()
}

/// When each eligible timed task starts next (for the panel).
pub fn next_due<Tz: chrono::TimeZone>(tasks: &[Task], paused: bool, now_ms: i64, tz: &Tz) -> Vec<(i64, i64)> {
    tasks
        .iter()
        .filter(|t| eligible(t, paused))
        .filter_map(|t| {
            let at = t.trigger.next_after(t.last_fire_ms.unwrap_or(now_ms), tz)?;
            // A missed start is caught up on the next tick.
            Some((t.id, at.max(now_ms)))
        })
        .collect()
}

/// Event-triggered tasks that `ev` starts, with their paths.
pub fn matching(tasks: &[Task], paused: bool, ev: &Event) -> Vec<(i64, Vec<String>)> {
    tasks
        .iter()
        .filter(|t| eligible(t, paused))
        .filter_map(|t| t.trigger.matches(ev).map(|p| (t.id, p)))
        .collect()
}

/// Is an event run allowed now, given the task's last start/end?
pub fn event_gap_ok(last_start_ms: Option<i64>, last_end_ms: Option<i64>, now_ms: i64) -> bool {
    let recent = |t: Option<i64>| t.is_some_and(|t| now_ms - t < EVENT_MIN_GAP_MS);
    !recent(last_start_ms) && !recent(last_end_ms)
}

/// Changes collected per watched folder until it has been quiet.
#[derive(Default, Debug)]
pub struct FolderBatches {
    pending: BTreeMap<String, (i64, Vec<String>)>,
}

impl FolderBatches {
    pub fn add(&mut self, folder: &str, path: String, now_ms: i64) {
        let e = self.pending.entry(folder.to_string()).or_insert((now_ms, Vec::new()));
        e.0 = now_ms;
        if !e.1.contains(&path) && e.1.len() < MAX_PATHS {
            e.1.push(path);
        }
    }

    /// Batches whose last change is at least [`FOLDER_QUIET_MS`] old.
    pub fn take_ready(&mut self, now_ms: i64) -> Vec<(String, Vec<String>)> {
        let ready: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, (last, _))| now_ms - last >= FOLDER_QUIET_MS)
            .map(|(k, _)| k.clone())
            .collect();
        ready
            .into_iter()
            .filter_map(|k| self.pending.remove(&k).map(|(_, paths)| (k, paths)))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// The folders event tasks want watched (eligible ones only — an unapproved
/// task must not even cost a watcher).
pub fn watched_folders(tasks: &[Task], paused: bool) -> HashSet<String> {
    tasks
        .iter()
        .filter(|t| eligible(t, paused))
        .filter_map(|t| match &t.trigger {
            Trigger::Folder { path } => Some(path.trim_end_matches(['/', '\\']).to_string()),
            _ => None,
        })
        .collect()
}

/// Skip noise a script should never be started for — and any path with a
/// control character: `IR_TASK_PATHS` is one path per line, so a file named
/// `x\n/Users/me/important` dropped into a watched folder would otherwise hand
/// the script a second, attacker-chosen path.
pub fn ignorable_path(path: &str) -> bool {
    if path.chars().any(char::is_control) {
        return true;
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name == ".DS_Store"
        || name.starts_with(".~")
        || name.ends_with(".crdownload")
        || name.ends_with(".download")
        || name.ends_with(".part")
        || name.ends_with(".tmp")
}

// ── Running ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Book {
    running: HashSet<i64>,
    last_start: HashMap<i64, i64>,
    last_end: HashMap<i64, i64>,
}

static BOOK: OnceLock<Mutex<Book>> = OnceLock::new();
fn book() -> &'static Mutex<Book> {
    BOOK.get_or_init(Default::default)
}

pub fn is_running(id: i64) -> bool {
    book().lock().running.contains(&id)
}

pub fn running_ids() -> Vec<i64> {
    book().lock().running.iter().copied().collect()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Start `task` on its own thread. Refused while it already runs or when it
/// isn't approved. `wait` = block until done and return the run (Run now).
pub fn start_run(
    app: Option<AppHandle>,
    db: DbHandle,
    task: Task,
    trigger_kind: &str,
    paths: Vec<String>,
    wait: bool,
) -> Result<Option<store::Run>, String> {
    if !task.approved {
        return Err("Dieser Task ist nicht freigegeben — erst das Skript ansehen und freigeben.".into());
    }
    let started = now_ms();
    {
        let mut b = book().lock();
        if !b.running.insert(task.id) {
            return Err("Dieser Task läuft gerade.".into());
        }
        b.last_start.insert(task.id, started);
    }
    let trigger_kind = trigger_kind.to_string();
    // The panel shows "running" right away.
    if let Some(app) = &app {
        let _ = app.emit("ai-tasks-changed", ());
    }
    let job = move || {
        let env = vec![
            ("IR_TASK_NAME".to_string(), task.name.clone()),
            ("IR_TASK_TRIGGER".to_string(), trigger_kind.clone()),
            ("IR_TASK_PATHS".to_string(), paths.join("\n")),
        ];
        let timeout = Duration::from_secs(u64::from(task.timeout_s));
        let result = runner::run_script(task.language, &task.script, &env, timeout);
        let run = store::record_run(&db, task.id, started, &trigger_kind, &result);
        {
            let mut b = book().lock();
            b.running.remove(&task.id);
            b.last_end.insert(task.id, now_ms());
        }
        match &result {
            Ok(o) if o.ok() => tracing::info!("ai task {} ({trigger_kind}) finished in {} ms", task.id, o.duration_ms),
            Ok(o) => tracing::warn!(
                "ai task {} ({trigger_kind}) ended: exit {:?}, timed out {}",
                task.id,
                o.exit_code,
                o.timed_out
            ),
            Err(e) => tracing::warn!("ai task {} ({trigger_kind}) could not start: {e}", task.id),
        }
        if let Some(app) = &app {
            let _ = app.emit("ai-tasks-changed", ());
        }
        run.ok()
    };
    if wait {
        Ok(job())
    } else {
        std::thread::Builder::new()
            .name("ir-ai-task-run".into())
            .spawn(move || {
                job();
            })
            .map_err(|e| e.to_string())?;
        Ok(None)
    }
}

// ── The loop ─────────────────────────────────────────────────────────────────

pub fn start(app: AppHandle, db: DbHandle) {
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = std::sync::mpsc::channel();
    if TX.set(Mutex::new(tx.clone())).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("ir-ai-tasks".into())
        .spawn(move || run_loop(app, db, rx, tx))
        .ok();
    // The app just started — tasks with that trigger run once.
    send(Msg::Event(Event::AppStart));
}

fn run_loop(app: AppHandle, db: DbHandle, rx: Receiver<Msg>, tx: Sender<Msg>) {
    let mut tasks: Vec<Task> = Vec::new();
    let mut paused = false;
    let mut watchers: HashMap<String, notify::RecommendedWatcher> = HashMap::new();
    let mut batches = FolderBatches::default();
    let mut needs_reload = true;
    let mut mono = Instant::now();
    let mut wall = SystemTime::now();

    loop {
        if needs_reload {
            tasks = store::list(&db).unwrap_or_default();
            paused = is_paused(&db);
            sync_watchers(&mut watchers, &watched_folders(&tasks, paused), &tx);
            needs_reload = false;
        }
        let wait = if batches.is_empty() { TICK } else { Duration::from_millis(500) };
        let msg = rx.recv_timeout(wait).ok();

        // Sleep detection: wall time advanced far beyond monotonic time.
        let (m_el, w_el) = (mono.elapsed(), wall.elapsed().unwrap_or_default());
        mono = Instant::now();
        wall = SystemTime::now();
        let woke = w_el > m_el + SLEPT_SLACK;

        let now = now_ms();
        let mut events: Vec<Event> = Vec::new();
        if woke {
            events.push(Event::Wake);
        }
        match msg {
            Some(Msg::Reload) => {
                needs_reload = true;
                continue;
            }
            Some(Msg::Event(ev)) => events.push(ev),
            Some(Msg::FolderChange { folder, path }) if !ignorable_path(&path) => {
                batches.add(&folder, path, now);
            }
            Some(Msg::FolderChange { .. }) => {}
            None => {}
        }
        for (folder, paths) in batches.take_ready(now) {
            events.push(Event::Folder { folder, paths });
        }

        for ev in &events {
            for (id, paths) in matching(&tasks, paused, ev) {
                let ok = {
                    let b = book().lock();
                    !b.running.contains(&id)
                        && event_gap_ok(b.last_start.get(&id).copied(), b.last_end.get(&id).copied(), now)
                };
                if !ok {
                    continue;
                }
                if let Some(t) = tasks.iter().find(|t| t.id == id).cloned() {
                    let kind = t.trigger.kind();
                    let _ = start_run(Some(app.clone()), db.clone(), t, kind, paths, false);
                }
            }
        }

        for id in due_now(&tasks, paused, now, &chrono::Local) {
            if is_running(id) {
                continue; // the next tick picks it up after this run ends
            }
            if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
                t.last_fire_ms = Some(now);
                let _ = store::set_last_fire(&db, id, now);
                let _ = start_run(Some(app.clone()), db.clone(), t.clone(), t.trigger.kind(), Vec::new(), false);
            }
        }
    }
}

fn sync_watchers(
    watchers: &mut HashMap<String, notify::RecommendedWatcher>,
    wanted: &HashSet<String>,
    tx: &Sender<Msg>,
) {
    watchers.retain(|k, _| wanted.contains(k));
    for folder in wanted {
        if watchers.contains_key(folder) {
            continue;
        }
        let tx = tx.clone();
        let f = folder.clone();
        let handler = move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                return;
            }
            for p in ev.paths {
                let _ = tx.send(Msg::FolderChange { folder: f.clone(), path: p.to_string_lossy().into_owned() });
            }
        };
        match notify::recommended_watcher(handler) {
            Ok(mut w) => match w.watch(std::path::Path::new(folder), RecursiveMode::NonRecursive) {
                Ok(()) => {
                    watchers.insert(folder.clone(), w);
                }
                Err(e) => tracing::warn!("ai tasks: cannot watch {folder}: {e}"),
            },
            Err(e) => tracing::warn!("ai tasks: watcher failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_tasks::generate::Language;
    use crate::ai_tasks::provider::ProviderId;
    use chrono::Utc;
    use std::sync::Arc;

    fn task(id: i64, trigger: Trigger, approved: bool, enabled: bool, last_fire: Option<i64>) -> Task {
        Task {
            id,
            name: format!("t{id}"),
            prompt: String::new(),
            language: Language::Bash,
            script: "echo".into(),
            explanation: String::new(),
            provider: ProviderId::Anthropic,
            trigger,
            enabled,
            timeout_s: 30,
            script_hash: "h".into(),
            approved,
            created_ms: 0,
            updated_ms: 0,
            last_fire_ms: last_fire,
            last_run: None,
        }
    }

    #[test]
    fn only_enabled_approved_non_manual_tasks_start_and_pause_stops_all() {
        let t = task(1, Trigger::Wake, true, true, None);
        assert!(eligible(&t, false));
        assert!(!eligible(&t, true), "the global pause wins");
        assert!(!eligible(&task(1, Trigger::Wake, false, true, None), false), "unapproved never runs");
        assert!(!eligible(&task(1, Trigger::Wake, true, false, None), false));
        assert!(!eligible(&task(1, Trigger::Manual, true, true, None), false));
    }

    #[test]
    fn interval_tasks_are_due_one_interval_after_their_last_fire() {
        let min = 60_000;
        let tasks = vec![
            task(1, Trigger::Interval { minutes: 10 }, true, true, Some(0)),
            task(2, Trigger::Interval { minutes: 10 }, true, true, None),
            task(3, Trigger::Interval { minutes: 10 }, false, true, Some(0)),
        ];
        assert!(due_now(&tasks, false, 9 * min, &Utc).is_empty());
        assert_eq!(due_now(&tasks, false, 10 * min, &Utc), vec![1], "never-fired counts from now, unapproved never");
        assert!(due_now(&tasks, true, 10 * min, &Utc).is_empty());
    }

    #[test]
    fn next_due_lists_only_eligible_timed_tasks_and_never_the_past() {
        let tasks = vec![
            task(1, Trigger::Interval { minutes: 10 }, true, true, Some(0)),
            task(2, Trigger::Wake, true, true, None),
            task(3, Trigger::Interval { minutes: 10 }, false, true, Some(0)),
        ];
        assert_eq!(next_due(&tasks, false, 60_000, &Utc), vec![(1, 600_000)]);
        assert_eq!(next_due(&tasks, false, 9_000_000, &Utc), vec![(1, 9_000_000)], "overdue = now");
        assert!(next_due(&tasks, true, 0, &Utc).is_empty());
    }

    #[test]
    fn a_missed_schedule_is_caught_up_once() {
        // Last fire long ago: due now, and after setting last_fire = now, not again.
        let t = vec![task(1, Trigger::Schedule { weekdays: vec![], hour: 3, minute: 0 }, true, true, Some(0))];
        let now = 10 * 24 * 3_600_000;
        assert_eq!(due_now(&t, false, now, &Utc), vec![1]);
        let t = vec![task(1, Trigger::Schedule { weekdays: vec![], hour: 3, minute: 0 }, true, true, Some(now))];
        assert!(due_now(&t, false, now + 1, &Utc).is_empty());
    }

    #[test]
    fn events_start_only_their_tasks() {
        let tasks = vec![
            task(1, Trigger::Wake, true, true, None),
            task(2, Trigger::AppStart, true, true, None),
            task(3, Trigger::Folder { path: "/x/in".into() }, true, true, None),
            task(4, Trigger::Folder { path: "/x/in".into() }, false, true, None),
        ];
        assert_eq!(matching(&tasks, false, &Event::Wake), vec![(1, vec![])]);
        let ev = Event::Folder { folder: "/x/in".into(), paths: vec!["/x/in/a".into()] };
        assert_eq!(matching(&tasks, false, &ev), vec![(3, vec!["/x/in/a".to_string()])]);
        assert!(matching(&tasks, true, &Event::Wake).is_empty());
    }

    #[test]
    fn event_runs_keep_a_gap_and_ignore_their_own_writes() {
        assert!(event_gap_ok(None, None, 0));
        assert!(!event_gap_ok(Some(1_000), None, 5_000), "too soon after the last start");
        assert!(!event_gap_ok(Some(0), Some(30_000), 35_000), "writes right after a run don't retrigger it");
        assert!(event_gap_ok(Some(0), Some(30_000), 30_000 + EVENT_MIN_GAP_MS));
    }

    #[test]
    fn folder_changes_are_batched_until_quiet() {
        let mut b = FolderBatches::default();
        b.add("/in", "/in/a".into(), 0);
        b.add("/in", "/in/a".into(), 500);
        b.add("/in", "/in/b".into(), 1_000);
        assert!(b.take_ready(2_500).is_empty(), "still changing");
        let ready = b.take_ready(1_000 + FOLDER_QUIET_MS);
        assert_eq!(ready, vec![("/in".to_string(), vec!["/in/a".to_string(), "/in/b".to_string()])]);
        assert!(b.is_empty());
    }

    #[test]
    fn a_batch_is_capped() {
        let mut b = FolderBatches::default();
        for i in 0..(MAX_PATHS + 50) {
            b.add("/in", format!("/in/{i}"), 0);
        }
        assert_eq!(b.take_ready(FOLDER_QUIET_MS)[0].1.len(), MAX_PATHS);
    }

    #[test]
    fn only_eligible_folder_tasks_are_watched() {
        let tasks = vec![
            task(1, Trigger::Folder { path: "/a/".into() }, true, true, None),
            task(2, Trigger::Folder { path: "/b".into() }, false, true, None),
            task(3, Trigger::Wake, true, true, None),
        ];
        assert_eq!(watched_folders(&tasks, false), HashSet::from(["/a".to_string()]));
        assert!(watched_folders(&tasks, true).is_empty());
    }

    #[test]
    fn download_leftovers_are_ignored() {
        for p in ["/d/.DS_Store", "/d/x.crdownload", "/d/x.part", "/d/.~lock"] {
            assert!(ignorable_path(p), "{p}");
        }
        assert!(!ignorable_path("/d/report.pdf"));
        // One path per line is the env contract: a newline in a file name
        // must never become a second path.
        assert!(ignorable_path("/d/x\n/Users/me/important"));
        assert!(ignorable_path("/d/a\rb"));
    }

    #[cfg(unix)]
    #[test]
    fn an_unapproved_task_is_refused_and_a_task_never_runs_twice_at_once() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        store::init_schema(&conn).unwrap();
        let db: DbHandle = Arc::new(Mutex::new(conn));
        let t = task(4242, Trigger::Manual, false, true, None);
        assert!(start_run(None, db.clone(), t, "manual", vec![], true).is_err());
        let mut slow = task(4243, Trigger::Manual, true, true, None);
        slow.script = "sleep 1\n".into();
        start_run(None, db.clone(), slow.clone(), "manual", vec![], false).unwrap();
        assert!(start_run(None, db.clone(), slow.clone(), "manual", vec![], false).is_err());
        std::thread::sleep(Duration::from_millis(1600));
        assert!(!is_running(4243));
        let run = start_run(None, db, slow, "manual", vec![], true).unwrap().unwrap();
        assert!(run.ok());
    }
}
