//! The AI providers that write task scripts: Anthropic (API key), the local
//! `claude` program (Claude Code, its own login), Gemini and OpenAI.
//!
//! Building a request and reading a response are pure (and tested against the
//! documented shapes); only `complete` touches the network or a subprocess.
//! API keys live in the OS keychain and are read at call time — they never
//! reach the frontend, the database or a log line.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Keychain service shared with the rest of the app (see `crypto.rs`).
const KEYRING_SERVICE: &str = "io.celox.inspector-rust";
const HTTP_TIMEOUT: Duration = Duration::from_secs(120);
/// Answer length cap — a script generator never needs more.
const MAX_TOKENS: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    /// Anthropic Messages API with an API key.
    Anthropic,
    /// The locally installed `claude` program (Claude Code) and its login.
    ClaudeCli,
    Gemini,
    #[serde(rename = "openai")]
    OpenAi,
}

impl ProviderId {
    pub const ALL: [ProviderId; 4] =
        [ProviderId::Anthropic, ProviderId::ClaudeCli, ProviderId::Gemini, ProviderId::OpenAi];

    pub fn label(self) -> &'static str {
        match self {
            ProviderId::Anthropic => "Claude (API-Schlüssel)",
            ProviderId::ClaudeCli => "Claude Code (lokal)",
            ProviderId::Gemini => "Gemini",
            ProviderId::OpenAi => "ChatGPT",
        }
    }

    /// Default model when the user hasn't picked one. Checked 2026-10.
    pub fn default_model(self) -> &'static str {
        match self {
            ProviderId::Anthropic => "claude-sonnet-5-5",
            // Empty = whatever Claude Code is configured to use.
            ProviderId::ClaudeCli => "",
            ProviderId::Gemini => "gemini-3.8-flash",
            ProviderId::OpenAi => "gpt-6.1-sol",
        }
    }

    /// Keychain account for the API key; `None` = no key (local program).
    pub fn key_account(self) -> Option<&'static str> {
        match self {
            ProviderId::Anthropic => Some("ai-key-anthropic-v1"),
            ProviderId::Gemini => Some("ai-key-gemini-v1"),
            ProviderId::OpenAi => Some("ai-key-openai-v1"),
            ProviderId::ClaudeCli => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderId::Anthropic => "anthropic",
            ProviderId::ClaudeCli => "claude_cli",
            ProviderId::Gemini => "gemini",
            ProviderId::OpenAi => "openai",
        }
    }

    pub fn parse(s: &str) -> Option<ProviderId> {
        ProviderId::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

// ── Keys (keychain) ──────────────────────────────────────────────────────────

pub fn set_key(p: ProviderId, key: &str) -> Result<(), String> {
    let account = p.key_account().ok_or("Dieser Anbieter braucht keinen Schlüssel.")?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, account).map_err(|e| e.to_string())?;
    let key = key.trim();
    if key.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(key).map_err(|e| format!("Schlüsselbund: {e}"))
}

pub fn get_key(p: ProviderId) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, p.key_account()?)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

/// Last four characters, for "a key is stored" in the UI — never the key.
pub fn key_hint(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() <= 8 {
        return "••••".into();
    }
    format!("••••{}", chars[chars.len() - 4..].iter().collect::<String>())
}

// ── Requests (pure) ──────────────────────────────────────────────────────────

/// One HTTP request, ready to send. Header VALUES may carry the key, so this
/// type deliberately has no `Debug`/`Serialize` — it must not end up in a log.
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

pub fn build_request(p: ProviderId, key: &str, model: &str, system: &str, user: &str) -> Option<HttpRequest> {
    match p {
        ProviderId::Anthropic => Some(HttpRequest {
            url: "https://api.anthropic.com/v1/messages".into(),
            headers: vec![
                ("x-api-key", key.into()),
                ("anthropic-version", "2023-06-01".into()),
                ("content-type", "application/json".into()),
            ],
            body: json!({
                "model": model,
                "max_tokens": MAX_TOKENS,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            }),
        }),
        ProviderId::Gemini => Some(HttpRequest {
            url: format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                url_segment(model)
            ),
            headers: vec![("x-goog-api-key", key.into()), ("content-type", "application/json".into())],
            body: json!({
                "systemInstruction": { "parts": [{ "text": system }] },
                "contents": [{ "role": "user", "parts": [{ "text": user }] }],
                "generationConfig": {
                    "responseMimeType": "application/json",
                    "maxOutputTokens": MAX_TOKENS,
                },
            }),
        }),
        ProviderId::OpenAi => Some(HttpRequest {
            url: "https://api.openai.com/v1/chat/completions".into(),
            headers: vec![
                ("authorization", format!("Bearer {key}")),
                ("content-type", "application/json".into()),
            ],
            body: json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
                "response_format": { "type": "json_object" },
            }),
        }),
        ProviderId::ClaudeCli => None,
    }
}

/// A model name goes into the Gemini URL path — keep it to safe characters.
fn url_segment(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
        .collect()
}

/// The answer text from a provider's JSON response.
pub fn parse_response(p: ProviderId, body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Antwort ist kein JSON.".to_string())?;
    let text = match p {
        ProviderId::Anthropic => v["content"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<String>()
            }),
        ProviderId::Gemini => v["candidates"][0]["content"]["parts"].as_array().map(|parts| {
            parts.iter().filter_map(|b| b["text"].as_str()).collect::<String>()
        }),
        ProviderId::OpenAi => v["choices"][0]["message"]["content"].as_str().map(String::from),
        ProviderId::ClaudeCli => {
            if v["is_error"].as_bool() == Some(true) {
                return Err(format!(
                    "Claude Code meldet einen Fehler: {}",
                    v["result"].as_str().unwrap_or("unbekannt")
                ));
            }
            v["result"].as_str().map(String::from)
        }
    };
    match text {
        Some(t) if !t.trim().is_empty() => Ok(t),
        _ => Err(provider_error(&v).unwrap_or_else(|| "Die Antwort enthält keinen Text.".into())),
    }
}

/// Error message a provider put into its JSON body, if any.
fn provider_error(v: &Value) -> Option<String> {
    v["error"]["message"]
        .as_str()
        .or_else(|| v["error"].as_str())
        .map(|m| m.chars().take(300).collect())
}

/// A user-facing message for an HTTP status, with the provider's own reason.
pub fn status_message(p: ProviderId, status: u16, body: &str) -> String {
    let reason = serde_json::from_str::<Value>(body).ok().and_then(|v| provider_error(&v));
    let what = match status {
        400 => "Anfrage abgelehnt (Modellname prüfen)",
        401 | 403 => "Schlüssel abgelehnt",
        404 => "Modell nicht gefunden",
        429 => "Kontingent oder Ratenlimit erreicht",
        500..=599 => "Dienst gerade gestört",
        _ => "Anfrage fehlgeschlagen",
    };
    match reason {
        Some(r) => format!("{}: {what} (HTTP {status}) — {r}", p.label()),
        None => format!("{}: {what} (HTTP {status})", p.label()),
    }
}

/// Arguments for the local `claude` program: print mode, JSON answer, NO tools
/// (it writes a script, it doesn't run anything), no saved session.
pub fn claude_cli_args(model: &str, system: &str) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--no-session-persistence",
        // Only the user's own settings — never project/local ones from the cwd.
        "--setting-sources",
        "user",
        "--tools",
        "",
        "--system-prompt",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(system.to_string());
    if !model.trim().is_empty() {
        a.push("--model".into());
        a.push(model.trim().to_string());
    }
    a
}

// ── Calls (impure) ───────────────────────────────────────────────────────────

/// Ask `p` and return the answer text.
pub fn complete(p: ProviderId, model: &str, system: &str, user: &str) -> Result<String, String> {
    let model = if model.trim().is_empty() { p.default_model() } else { model.trim() };
    if p == ProviderId::ClaudeCli {
        return complete_cli(model, system, user);
    }
    let key = get_key(p).ok_or_else(|| format!("Für {} ist kein API-Schlüssel hinterlegt.", p.label()))?;
    let req = build_request(p, &key, model, system, user).ok_or("kein HTTP-Anbieter")?;
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build();
    let mut call = agent.post(&req.url);
    for (k, v) in &req.headers {
        call = call.set(k, v);
    }
    match call.send_string(&req.body.to_string()) {
        Ok(resp) => {
            let body = resp.into_string().map_err(|e| format!("Antwort nicht lesbar: {e}"))?;
            parse_response(p, &body)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Err(status_message(p, code, &body))
        }
        Err(ureq::Error::Transport(t)) => Err(format!("{}: nicht erreichbar ({})", p.label(), t.kind())),
    }
}

/// Where the local `claude` program lives — GUI apps don't inherit the shell PATH.
pub fn claude_binary() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir();
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(h) = &home {
        candidates.push(h.join(".local/bin/claude"));
        candidates.push(h.join(".claude/local/claude"));
        candidates.push(h.join(".npm-global/bin/claude"));
    }
    candidates.push("/opt/homebrew/bin/claude".into());
    candidates.push("/usr/local/bin/claude".into());
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(if cfg!(windows) { "claude.cmd" } else { "claude" }));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// A private, empty working folder for `claude`. Claude Code reads project
/// settings (CLAUDE.md, .claude/) from where it starts, so a shared path like
/// `/tmp/…` would let another local user plant instructions there. Per-user
/// cache dir, mode 0700, emptied before every call.
fn private_cli_dir() -> Result<std::path::PathBuf, String> {
    let dir = dirs::cache_dir()
        .ok_or("Kein Cache-Ordner gefunden.")?
        .join("InspectorRust")
        .join("ai-cwd");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Arbeitsordner für Claude Code: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Arbeitsordner für Claude Code: {e}"))?;
    }
    Ok(dir)
}

fn complete_cli(model: &str, system: &str, user: &str) -> Result<String, String> {
    let bin = claude_binary().ok_or("Claude Code ist nicht installiert (Programm `claude` nicht gefunden).")?;
    // An empty working folder: Claude Code reads project files (CLAUDE.md …)
    // from where it starts, and none of the user's projects belong in here.
    let dir = private_cli_dir()?;
    let mut child = Command::new(bin)
        .args(claude_cli_args(model, system))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Claude Code ließ sich nicht starten: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(user.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = super::runner::wait_with_timeout(child, HTTP_TIMEOUT)
        .map_err(|e| format!("Claude Code: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "Claude Code hat nichts geantwortet{}",
            if err.trim().is_empty() { String::new() } else { format!(": {}", err.trim().chars().take(300).collect::<String>()) }
        ));
    }
    parse_response(ProviderId::ClaudeCli, &stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_serialize_as_the_settings_store_them() {
        for p in ProviderId::ALL {
            assert_eq!(ProviderId::parse(p.as_str()), Some(p));
            assert_eq!(serde_json::to_value(p).unwrap(), Value::String(p.as_str().into()));
        }
        assert_eq!(ProviderId::parse("chatgpt"), None);
    }

    #[test]
    fn only_the_api_providers_have_a_keychain_slot() {
        assert!(ProviderId::ClaudeCli.key_account().is_none());
        let slots: Vec<_> = ProviderId::ALL.iter().filter_map(|p| p.key_account()).collect();
        assert_eq!(slots.len(), 3);
        let mut dedup = slots.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), 3, "each provider its own slot");
    }

    #[test]
    fn the_hint_never_shows_more_than_the_last_four() {
        assert_eq!(key_hint("sk-ant-api03-abcdefWXYZ"), "••••WXYZ");
        assert_eq!(key_hint("short"), "••••", "a short value would give most of it away");
    }

    #[test]
    fn anthropic_request_carries_key_version_system_and_user() {
        let r = build_request(ProviderId::Anthropic, "K", "m", "SYS", "USER").unwrap();
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.contains(&("x-api-key", "K".into())));
        assert!(r.headers.iter().any(|(k, _)| *k == "anthropic-version"));
        assert_eq!(r.body["system"], "SYS");
        assert_eq!(r.body["messages"][0]["content"], "USER");
        assert_eq!(r.body["model"], "m");
    }

    #[test]
    fn gemini_keeps_the_key_out_of_the_url_and_sanitises_the_model() {
        let r = build_request(ProviderId::Gemini, "SECRET", "gemini-3.8-flash/../x?y", "S", "U").unwrap();
        assert!(!r.url.contains("SECRET"), "a key in the URL ends up in proxies and logs");
        assert!(r.url.ends_with("/models/gemini-3.8-flash..xy:generateContent"));
        assert!(r.headers.contains(&("x-goog-api-key", "SECRET".into())));
        assert_eq!(r.body["systemInstruction"]["parts"][0]["text"], "S");
    }

    #[test]
    fn openai_uses_bearer_and_json_mode() {
        let r = build_request(ProviderId::OpenAi, "K", "m", "S", "U").unwrap();
        assert!(r.headers.contains(&("authorization", "Bearer K".into())));
        assert_eq!(r.body["messages"][0]["role"], "system");
        assert_eq!(r.body["response_format"]["type"], "json_object");
    }

    #[test]
    fn the_local_program_has_no_http_request() {
        assert!(build_request(ProviderId::ClaudeCli, "", "", "", "").is_none());
    }

    #[test]
    fn responses_are_read_per_provider() {
        let a = r#"{"content":[{"type":"text","text":"he"},{"type":"tool_use"},{"type":"text","text":"llo"}]}"#;
        assert_eq!(parse_response(ProviderId::Anthropic, a).unwrap(), "hello");
        let g = r#"{"candidates":[{"content":{"parts":[{"text":"{\"a\":1}"}]}}]}"#;
        assert_eq!(parse_response(ProviderId::Gemini, g).unwrap(), r#"{"a":1}"#);
        let o = r#"{"choices":[{"message":{"content":"x"}}]}"#;
        assert_eq!(parse_response(ProviderId::OpenAi, o).unwrap(), "x");
        let c = r#"{"is_error":false,"result":"ok","usage":{}}"#;
        assert_eq!(parse_response(ProviderId::ClaudeCli, c).unwrap(), "ok");
    }

    #[test]
    fn empty_and_error_answers_are_errors_not_empty_scripts() {
        assert!(parse_response(ProviderId::OpenAi, r#"{"choices":[{"message":{"content":"  "}}]}"#).is_err());
        let e = parse_response(ProviderId::Anthropic, r#"{"error":{"message":"overloaded"}}"#).unwrap_err();
        assert!(e.contains("overloaded"));
        assert!(parse_response(ProviderId::ClaudeCli, r#"{"is_error":true,"result":"Login"}"#)
            .unwrap_err()
            .contains("Login"));
        assert!(parse_response(ProviderId::Gemini, "<html>").is_err());
    }

    #[test]
    fn status_messages_name_the_cause_and_the_providers_reason() {
        let m = status_message(ProviderId::Gemini, 403, r#"{"error":{"message":"API key not valid"}}"#);
        assert!(m.contains("Schlüssel abgelehnt") && m.contains("API key not valid") && m.contains("Gemini"));
        assert!(status_message(ProviderId::OpenAi, 429, "").contains("Ratenlimit"));
    }

    #[test]
    fn the_local_program_runs_without_tools_and_without_a_saved_session() {
        let a = claude_cli_args("", "SYS");
        let tools = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[tools + 1], "", "no tools: it writes a script, it runs nothing");
        assert!(a.contains(&"--no-session-persistence".to_string()));
        let src = a.iter().position(|x| x == "--setting-sources").unwrap();
        assert_eq!(a[src + 1], "user", "project settings from the cwd must never be loaded");
        assert!(!a.contains(&"--model".to_string()), "empty model = Claude Code's own default");
        let b = claude_cli_args("sonnet", "SYS");
        assert_eq!(&b[b.len() - 2..], &["--model".to_string(), "sonnet".to_string()]);
    }
}
