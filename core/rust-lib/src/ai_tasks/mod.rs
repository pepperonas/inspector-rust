//! AI tasks: scripts an AI writes from a description, which run on their own —
//! on an interval, on a schedule, or when something happens (a folder changes,
//! the app starts, the computer wakes).
//!
//! The safety model is the point of the design:
//! * a script runs only in the exact version the user reviewed and approved
//!   ([`store::approve`] takes the hash of what was shown; any edit withdraws it);
//! * generation never runs anything — it returns a draft;
//! * every run has a time limit and is killed with its children when it overruns;
//! * one global switch pauses all tasks.
//!
//! Layout: `provider` (the three AI vendors + local Claude Code), `generate`
//! (prompt + strict reading of the answer), `trigger` (when), `runner` (how),
//! `store` (database), `scheduler` (the background thread).

pub mod generate;
pub mod ipc;
pub mod provider;
pub mod runner;
pub mod scheduler;
pub mod store;
pub mod trigger;

use crate::db::DbHandle;
use crate::settings;
use anyhow::Result;
use generate::{Generated, Language};
use provider::ProviderId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const KEY_PROVIDER: &str = "ai.provider";
pub const KEY_MODEL_PREFIX: &str = "ai.model.";
pub const KEY_PAUSED: &str = "aitasks.paused";

pub const ALL_PROVIDERS: [ProviderId; 4] =
    [ProviderId::Anthropic, ProviderId::ClaudeCli, ProviderId::Gemini, ProviderId::OpenAi];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiConfig {
    /// The provider new tasks are generated with.
    pub provider: ProviderId,
    /// Model per provider (`as_str` → model id). Missing = provider default.
    #[serde(default)]
    pub models: BTreeMap<String, String>,
}

impl AiConfig {
    pub fn model_for(&self, p: ProviderId) -> String {
        self.models
            .get(p.as_str())
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| p.default_model().to_string())
    }
}

pub fn get_config(db: &DbHandle) -> AiConfig {
    let provider = settings::get(db, KEY_PROVIDER)
        .ok()
        .flatten()
        .and_then(|s| ProviderId::parse(&s))
        .unwrap_or(ProviderId::Anthropic);
    let mut models = BTreeMap::new();
    for p in ALL_PROVIDERS {
        let m = settings::get(db, &format!("{KEY_MODEL_PREFIX}{}", p.as_str()))
            .ok()
            .flatten()
            .unwrap_or_default();
        models.insert(p.as_str().to_string(), if m.trim().is_empty() { p.default_model().to_string() } else { m });
    }
    AiConfig { provider, models }
}

pub fn set_config(db: &DbHandle, cfg: &AiConfig) -> Result<()> {
    settings::set(db, KEY_PROVIDER, cfg.provider.as_str())?;
    for p in ALL_PROVIDERS {
        let m = cfg.models.get(p.as_str()).map(|s| s.trim()).unwrap_or("");
        // Storing the default as empty keeps later default updates reaching the user.
        let stored = if m == p.default_model() { "" } else { m };
        settings::set(db, &format!("{KEY_MODEL_PREFIX}{}", p.as_str()), stored)?;
    }
    Ok(())
}

pub fn is_paused(db: &DbHandle) -> bool {
    settings::get_bool(db, KEY_PAUSED, false).unwrap_or(false)
}

pub fn set_paused(db: &DbHandle, paused: bool) -> Result<()> {
    settings::set(db, KEY_PAUSED, if paused { "true" } else { "false" })
}

/// Per-provider readiness for the settings page — never the key itself.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatus {
    pub id: ProviderId,
    pub label: String,
    pub configured: bool,
    /// `••••abcd` for a stored key, the program path for Claude Code.
    pub hint: Option<String>,
    pub model: String,
    pub default_model: String,
}

pub fn provider_status(db: &DbHandle) -> Vec<ProviderStatus> {
    let cfg = get_config(db);
    ALL_PROVIDERS
        .iter()
        .map(|&p| {
            let (configured, hint) = if p == ProviderId::ClaudeCli {
                let bin = provider::claude_binary();
                (bin.is_some(), bin.map(|b| b.display().to_string()))
            } else {
                match provider::get_key(p) {
                    Some(k) => (true, Some(provider::key_hint(&k))),
                    None => (false, None),
                }
            };
            ProviderStatus {
                id: p,
                label: p.label().to_string(),
                configured,
                hint,
                model: cfg.model_for(p),
                default_model: p.default_model().to_string(),
            }
        })
        .collect()
}

/// Ask the AI for a script. Runs nothing; the result is an unsaved draft.
pub fn generate(
    db: &DbHandle,
    provider: ProviderId,
    description: &str,
    previous: Option<(Language, String)>,
    feedback: Option<&str>,
) -> Result<Generated, String> {
    if description.trim().is_empty() {
        return Err("Beschreibe zuerst, was der Task tun soll.".into());
    }
    let model = get_config(db).model_for(provider);
    let allowed = generate::allowed_languages();
    let system = generate::system_prompt(allowed);
    let user = generate::user_prompt(description, previous.as_ref().map(|(l, s)| (l, s.as_str())), feedback);
    let text = provider::complete(provider, &model, &system, &user)?;
    generate::parse_generated(&text, allowed)
}

/// A minimal round trip to check a key/model: ask for one word.
pub fn test_provider(db: &DbHandle, provider: ProviderId) -> Result<String, String> {
    let model = get_config(db).model_for(provider);
    let answer = provider::complete(
        provider,
        &model,
        "Antworte mit genau einem Wort.",
        "Sag „bereit“.",
    )?;
    Ok(format!("{} antwortet ({model}): {}", provider.label(), answer.trim().chars().take(40).collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn db() -> DbHandle {
        let db: DbHandle = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn config_defaults_and_round_trips() {
        let db = db();
        let c = get_config(&db);
        assert_eq!(c.provider, ProviderId::Anthropic);
        for p in ALL_PROVIDERS {
            assert_eq!(c.model_for(p), p.default_model());
        }
        let mut c2 = c.clone();
        c2.provider = ProviderId::Gemini;
        c2.models.insert("gemini".into(), "gemini-x".into());
        set_config(&db, &c2).unwrap();
        let back = get_config(&db);
        assert_eq!(back.provider, ProviderId::Gemini);
        assert_eq!(back.model_for(ProviderId::Gemini), "gemini-x");
        assert_eq!(back.model_for(ProviderId::OpenAi), ProviderId::OpenAi.default_model());
    }

    #[test]
    fn the_default_model_is_stored_as_empty_so_updates_reach_the_user() {
        let db = db();
        set_config(&db, &get_config(&db)).unwrap();
        assert_eq!(settings::get(&db, "ai.model.anthropic").unwrap().as_deref(), Some(""));
    }

    #[test]
    fn pause_round_trips() {
        let db = db();
        assert!(!is_paused(&db));
        set_paused(&db, true).unwrap();
        assert!(is_paused(&db));
    }

    #[test]
    fn an_empty_description_is_refused_before_any_network_call() {
        let db = db();
        assert!(generate(&db, ProviderId::OpenAi, "  ", None, None).is_err());
    }

    /// Live: the local Claude Code writes a script, it parses, and it runs.
    /// `cargo test -p inspector-rust-core --lib ai_tasks_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ai_tasks_live_claude_cli_writes_a_runnable_script() {
        let db = db();
        let g = generate(
            &db,
            ProviderId::ClaudeCli,
            "Gib die Anzahl der Dateien in meinem Home-Verzeichnis (nur oberste Ebene) aus.",
            None,
            None,
        )
        .expect("generate");
        println!("{} [{}]\n{}\n-- {}", g.name, g.language.as_str(), g.script, g.explanation);
        let out = runner::run_script(g.language, &g.script, &[], std::time::Duration::from_secs(30)).unwrap();
        println!("exit {:?}: {}", out.exit_code, out.output);
        assert!(out.ok());
    }
}
