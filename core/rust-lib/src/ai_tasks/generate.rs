//! Turning a description into a script: the prompt we send and the strict
//! reading of what comes back. Pure — the network call lives in `provider`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Upper bound for a generated script; a task that needs more isn't a task.
pub const MAX_SCRIPT_BYTES: usize = 64 * 1024;
pub const MAX_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Zsh,
    Bash,
    Python,
    #[serde(rename = "applescript")]
    AppleScript,
    #[serde(rename = "powershell")]
    PowerShell,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Zsh => "zsh",
            Language::Bash => "bash",
            Language::Python => "python",
            Language::AppleScript => "applescript",
            Language::PowerShell => "powershell",
        }
    }

    pub fn parse(s: &str) -> Option<Language> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zsh" => Some(Language::Zsh),
            "bash" | "sh" | "shell" => Some(Language::Bash),
            "python" | "python3" | "py" => Some(Language::Python),
            "applescript" | "osascript" => Some(Language::AppleScript),
            "powershell" | "pwsh" | "ps1" => Some(Language::PowerShell),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Language::Zsh => "zsh",
            Language::Bash => "sh",
            Language::Python => "py",
            Language::AppleScript => "applescript",
            Language::PowerShell => "ps1",
        }
    }
}

/// The languages a script may use on this OS.
pub fn allowed_languages() -> &'static [Language] {
    if cfg!(target_os = "macos") {
        &[Language::Zsh, Language::Bash, Language::Python, Language::AppleScript]
    } else if cfg!(windows) {
        &[Language::PowerShell, Language::Python]
    } else {
        &[Language::Bash, Language::Zsh, Language::Python]
    }
}

fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    }
}

/// What the AI produced, after validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generated {
    pub name: String,
    pub language: Language,
    pub script: String,
    pub explanation: String,
}

/// The system prompt. It pins the answer to one JSON object and spells out the
/// runtime the script will meet — the environment variables are a contract
/// with `runner.rs`.
pub fn system_prompt(allowed: &[Language]) -> String {
    let langs = allowed.iter().map(|l| l.as_str()).collect::<Vec<_>>().join(", ");
    format!(
        "Du schreibst kleine Automatisierungs-Skripte für {os}, die eine App unbeaufsichtigt ausführt.\n\
         Antworte AUSSCHLIESSLICH mit einem JSON-Objekt, ohne Markdown, ohne Text davor oder danach:\n\
         {{\"name\": \"kurzer Name (max. {max} Zeichen)\", \"language\": \"eine von: {langs}\", \
         \"script\": \"das vollständige Skript\", \"explanation\": \"2–4 Sätze auf Deutsch: was es tut, was es verändert\"}}\n\
         Wähle die Sprache, die für die Aufgabe am besten passt.\n\
         Regeln für das Skript:\n\
         - Läuft ohne Rückfragen, ohne Eingaben, ohne sudo und ohne Passwort.\n\
         - Arbeitsverzeichnis ist das Home-Verzeichnis. Verwende absolute Pfade oder $HOME.\n\
         - Verfügbare Umgebungsvariablen: IR_TASK_NAME (Name des Tasks), IR_TASK_TRIGGER \
         (interval | schedule | folder | app_start | wake | manual), IR_TASK_PATHS (bei folder: \
         geänderte Pfade, eine Zeile je Pfad).\n\
         - Ausgaben auf stdout/stderr werden protokolliert; der Exit-Code 0 bedeutet Erfolg.\n\
         - Es gibt eine Laufzeitgrenze; das Skript soll zügig fertig werden und nicht dauerhaft laufen.\n\
         - Nichts löschen oder überschreiben, was die Beschreibung nicht ausdrücklich verlangt. \
         Wenn doch: lieber in den Papierkorb verschieben als endgültig löschen.\n\
         - Mehrfaches Ausführen darf nichts kaputt machen.\n\
         - Keine Platzhalter: das Skript muss so wie es ist funktionieren.",
        os = os_name(),
        max = MAX_NAME_CHARS,
    )
}

/// The user message: the description, and — when revising — the current
/// script plus what should change.
pub fn user_prompt(description: &str, previous: Option<(&Language, &str)>, feedback: Option<&str>) -> String {
    let mut s = format!("Aufgabe:\n{}\n", description.trim());
    if let Some((lang, script)) = previous {
        s.push_str(&format!(
            "\nBisheriges Skript ({}):\n{}\n",
            lang.as_str(),
            script
        ));
    }
    if let Some(f) = feedback.map(str::trim).filter(|f| !f.is_empty()) {
        s.push_str(&format!("\nÄnderungswunsch:\n{f}\n"));
    }
    s
}

/// The first complete JSON object in `text` — models wrap their answer in
/// ```json fences or add a sentence despite the instruction.
fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Read and validate the AI's answer. Strict on purpose: a script that ends
/// up runnable has to come through here.
pub fn parse_generated(text: &str, allowed: &[Language]) -> Result<Generated, String> {
    let json = extract_json(text).ok_or("Die KI hat kein JSON geliefert.")?;
    let v: Value = serde_json::from_str(json).map_err(|e| format!("Die Antwort ist kein gültiges JSON: {e}"))?;
    let field = |k: &str| v[k].as_str().map(str::trim).unwrap_or("");

    let lang_raw = field("language");
    let language = Language::parse(lang_raw)
        .ok_or_else(|| format!("Unbekannte Skriptsprache „{lang_raw}“."))?;
    if !allowed.contains(&language) {
        return Err(format!("Die Sprache „{}“ ist auf diesem System nicht erlaubt.", language.as_str()));
    }
    let script = v["script"].as_str().unwrap_or("").trim_matches('\n');
    if script.trim().is_empty() {
        return Err("Die KI hat ein leeres Skript geliefert.".into());
    }
    if script.len() > MAX_SCRIPT_BYTES {
        return Err("Das Skript ist zu lang (über 64 KB).".into());
    }
    let mut name: String = field("name").chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        name = "Neuer Task".into();
    }
    Ok(Generated {
        name,
        language,
        script: format!("{script}\n"),
        explanation: field("explanation").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: &[Language] = &[Language::Zsh, Language::Bash, Language::Python, Language::AppleScript];

    #[test]
    fn languages_parse_their_common_spellings() {
        assert_eq!(Language::parse("Python3"), Some(Language::Python));
        assert_eq!(Language::parse("sh"), Some(Language::Bash));
        assert_eq!(Language::parse("osascript"), Some(Language::AppleScript));
        assert_eq!(Language::parse("ruby"), None);
        for l in [Language::Zsh, Language::Bash, Language::Python, Language::AppleScript, Language::PowerShell] {
            assert_eq!(Language::parse(l.as_str()), Some(l));
            assert_eq!(serde_json::to_value(l).unwrap(), Value::String(l.as_str().into()));
        }
    }

    #[test]
    fn a_plain_answer_is_read() {
        let g = parse_generated(
            r#"{"name":"Downloads aufräumen","language":"zsh","script":"echo hi","explanation":"Sagt hallo."}"#,
            MAC,
        )
        .unwrap();
        assert_eq!(g.language, Language::Zsh);
        assert_eq!(g.script, "echo hi\n");
        assert_eq!(g.name, "Downloads aufräumen");
    }

    #[test]
    fn fences_and_chatter_around_the_json_are_tolerated() {
        let text = "Klar! Hier:\n```json\n{\"name\":\"x\",\"language\":\"python\",\"script\":\"print('{}')\\n\",\"explanation\":\"\"}\n```\nViel Spaß";
        let g = parse_generated(text, MAC).unwrap();
        assert_eq!(g.script, "print('{}')\n", "braces inside strings don't end the object");
    }

    #[test]
    fn a_language_not_allowed_here_is_rejected() {
        let e = parse_generated(r#"{"language":"powershell","script":"x"}"#, MAC).unwrap_err();
        assert!(e.contains("nicht erlaubt"));
        assert!(parse_generated(r#"{"language":"ruby","script":"x"}"#, MAC).is_err());
    }

    #[test]
    fn empty_missing_or_huge_scripts_are_rejected() {
        assert!(parse_generated(r#"{"language":"zsh","script":"  \n"}"#, MAC).is_err());
        assert!(parse_generated(r#"{"language":"zsh"}"#, MAC).is_err());
        let big = "x".repeat(MAX_SCRIPT_BYTES + 1);
        assert!(parse_generated(&format!(r#"{{"language":"zsh","script":"{big}"}}"#), MAC).is_err());
        assert!(parse_generated("gar kein json", MAC).is_err());
    }

    #[test]
    fn names_are_capped_and_never_empty() {
        let long = "n".repeat(200);
        let g = parse_generated(&format!(r#"{{"name":"{long}","language":"zsh","script":"x"}}"#), MAC).unwrap();
        assert_eq!(g.name.chars().count(), MAX_NAME_CHARS);
        let g = parse_generated(r#"{"language":"zsh","script":"x"}"#, MAC).unwrap();
        assert_eq!(g.name, "Neuer Task");
    }

    #[test]
    fn the_prompt_names_the_allowed_languages_and_the_runner_contract() {
        let s = system_prompt(MAC);
        assert!(s.contains("zsh, bash, python, applescript"));
        for var in ["IR_TASK_NAME", "IR_TASK_TRIGGER", "IR_TASK_PATHS"] {
            assert!(s.contains(var), "{var} is set by runner.rs and must be announced");
        }
    }

    #[test]
    fn a_revision_carries_the_old_script_and_the_change() {
        let u = user_prompt("Aufräumen", Some((&Language::Zsh, "echo alt")), Some("auch Bilder"));
        assert!(u.contains("echo alt") && u.contains("auch Bilder") && u.contains("Aufräumen"));
        let fresh = user_prompt("Aufräumen", None, Some("  "));
        assert!(!fresh.contains("Änderungswunsch"));
    }
}
