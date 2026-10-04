//! Tauri commands for AI tasks. Network calls and script runs block, so every
//! command that touches them runs on the blocking pool — a sync command would
//! run ON the main thread and freeze the UI.

use super::generate::{Generated, Language};
use super::provider::ProviderId;
use super::scheduler;
use super::store::{self, Run, Task, TaskDraft};
use super::{AiConfig, ProviderStatus};
use crate::db::DbHandle;
use tauri::{AppHandle, Emitter, State};

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn changed(app: &AppHandle) {
    scheduler::reload();
    let _ = app.emit("ai-tasks-changed", ());
}

#[derive(serde::Serialize)]
pub struct AiTasksState {
    pub tasks: Vec<Task>,
    pub running: Vec<i64>,
    pub paused: bool,
    /// Languages scripts may use on this OS.
    pub languages: Vec<Language>,
    /// task id → next scheduled start, for eligible timed tasks.
    pub next_due: std::collections::BTreeMap<String, i64>,
}

#[tauri::command]
pub async fn ai_provider_status(db: State<'_, DbHandle>) -> Result<Vec<ProviderStatus>, String> {
    let db = (*db).clone();
    // Keychain reads can prompt / block briefly.
    blocking(move || Ok(super::provider_status(&db))).await
}

#[tauri::command]
pub async fn ai_set_key(provider: ProviderId, key: String) -> Result<(), String> {
    blocking(move || super::provider::set_key(provider, &key)).await
}

#[tauri::command]
pub async fn ai_test_provider(db: State<'_, DbHandle>, provider: ProviderId) -> Result<String, String> {
    let db = (*db).clone();
    blocking(move || super::test_provider(&db, provider)).await
}

#[tauri::command]
pub fn ai_get_config(db: State<'_, DbHandle>) -> AiConfig {
    super::get_config(&db)
}

#[tauri::command]
pub fn ai_set_config(db: State<'_, DbHandle>, config: AiConfig) -> Result<(), String> {
    super::set_config(&db, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ai_tasks_state(db: State<'_, DbHandle>) -> Result<AiTasksState, String> {
    let tasks = store::list(&db).map_err(|e| e.to_string())?;
    let paused = super::is_paused(&db);
    let next_due = scheduler::next_due(&tasks, paused, now(), &chrono::Local)
        .into_iter()
        .map(|(id, ms)| (id.to_string(), ms))
        .collect();
    Ok(AiTasksState {
        tasks,
        running: scheduler::running_ids(),
        paused,
        languages: super::generate::allowed_languages().to_vec(),
        next_due,
    })
}

/// Ask the AI for a script (a draft — nothing is saved or run).
#[tauri::command]
pub async fn ai_task_generate(
    db: State<'_, DbHandle>,
    provider: ProviderId,
    description: String,
    previous_language: Option<Language>,
    previous_script: Option<String>,
    feedback: Option<String>,
) -> Result<Generated, String> {
    let db = (*db).clone();
    blocking(move || {
        let previous = match (previous_language, previous_script) {
            (Some(l), Some(s)) if !s.trim().is_empty() => Some((l, s)),
            _ => None,
        };
        super::generate(&db, provider, &description, previous, feedback.as_deref())
    })
    .await
}

#[tauri::command]
pub fn ai_task_save(app: AppHandle, db: State<'_, DbHandle>, draft: TaskDraft) -> Result<Task, String> {
    let t = store::save(&db, &draft, now()).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(t)
}

/// Approve the version with `hash` — the one the user was shown.
#[tauri::command]
pub fn ai_task_approve(app: AppHandle, db: State<'_, DbHandle>, id: i64, hash: String) -> Result<Task, String> {
    let t = store::approve(&db, id, &hash, now()).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(t)
}

#[tauri::command]
pub fn ai_task_revoke(app: AppHandle, db: State<'_, DbHandle>, id: i64) -> Result<(), String> {
    store::revoke(&db, id).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn ai_task_set_enabled(app: AppHandle, db: State<'_, DbHandle>, id: i64, enabled: bool) -> Result<(), String> {
    store::set_enabled(&db, id, enabled, now()).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn ai_task_delete(app: AppHandle, db: State<'_, DbHandle>, id: i64) -> Result<(), String> {
    store::delete(&db, id).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn ai_tasks_set_paused(app: AppHandle, db: State<'_, DbHandle>, paused: bool) -> Result<(), String> {
    super::set_paused(&db, paused).map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

/// Run now and wait for the result (the panel shows it).
#[tauri::command]
pub async fn ai_task_run_now(app: AppHandle, db: State<'_, DbHandle>, id: i64) -> Result<Option<Run>, String> {
    let db = (*db).clone();
    blocking(move || {
        let task = store::get(&db, id).map_err(|e| e.to_string())?.ok_or("Task nicht gefunden.")?;
        scheduler::start_run(Some(app), db, task, "manual", Vec::new(), true)
    })
    .await
}

#[tauri::command]
pub fn ai_task_runs(db: State<'_, DbHandle>, id: i64, limit: Option<i64>) -> Result<Vec<Run>, String> {
    store::runs(&db, id, limit.unwrap_or(20)).map_err(|e| e.to_string())
}
