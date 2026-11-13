//! The OS keychain, in one place: macOS Keychain, Windows Credential Manager.
//!
//! ⚠️ The `keyring` crate stores NOTHING unless a platform feature is enabled
//! (`apple-native`, `windows-native` in the workspace manifest). Without one
//! it falls back to a mock that keeps a value in memory, per `Entry` object:
//! `set_password` reports success and the next `Entry` finds nothing. The app
//! shipped that way until v0.254.0 — the backup password, the device-sync
//! passphrase, the GitHub tokens and the AI keys were never stored (found
//! 2026-11-12: "The password could not be stored in the keychain").
//! [`set`] therefore reads back what it wrote and fails when it is not there.
//!
//! Linux has no feature enabled (the Secret Service needs D-Bus at build
//! time): secrets there still live only until the app quits, and [`set`]
//! says so instead of pretending.
//!
//! Tests never touch the real keychain: under `cfg(test)` the store is a map
//! in the process.

#[cfg_attr(test, allow(dead_code))]
pub const SERVICE: &str = "io.celox.inspector-rust";

/// What the keychain says about an account.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Found(String),
    /// The ONLY answer that means "there is nothing here".
    Absent,
    /// The keychain could not be read — something may well be stored.
    Failed(String),
}

#[cfg(test)]
fn store() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

#[cfg(test)]
pub fn lookup(account: &str) -> Lookup {
    match store().lock().unwrap().get(account) {
        Some(v) => Lookup::Found(v.clone()),
        None => Lookup::Absent,
    }
}

/// What this process already learned about each account. macOS asks the user
/// for the login password on every read of an item the running build has not
/// been allowed yet (and every new build counts as a new app for a
/// self-signed bundle) — so each account is read from the keychain at most
/// ONCE per run; a refusal is remembered too instead of asking again.
#[cfg(not(test))]
fn cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, Lookup>> {
    static C: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Lookup>>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

#[cfg(not(test))]
pub fn lookup(account: &str) -> Lookup {
    // The lock is held across the read: two threads asking for the same
    // account must not raise two password dialogs.
    let mut known = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = known.get(account) {
        return hit.clone();
    }
    let fresh = read_keychain_item(account);
    known.insert(account.to_string(), fresh.clone());
    fresh
}

#[cfg(not(test))]
fn read_keychain_item(account: &str) -> Lookup {
    let entry = match keyring::Entry::new(SERVICE, account) {
        Ok(e) => e,
        Err(e) => return Lookup::Failed(format!("entry: {e}")),
    };
    match entry.get_password() {
        Ok(s) => Lookup::Found(s),
        Err(keyring::Error::NoEntry) => Lookup::Absent,
        Err(e) => Lookup::Failed(e.to_string()),
    }
}

/// The stored secret, if there is a non-blank one.
pub fn get(account: &str) -> Option<String> {
    match lookup(account) {
        Lookup::Found(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

#[cfg(test)]
fn write(account: &str, value: &str) -> Result<(), String> {
    store().lock().unwrap().insert(account.to_string(), value.to_string());
    Ok(())
}

#[cfg(not(test))]
fn write(account: &str, value: &str) -> Result<(), String> {
    keyring::Entry::new(SERVICE, account)
        .and_then(|e| e.set_password(value))
        .map_err(|e| e.to_string())?;
    // Re-read once (the write check in `set`), then answer from memory.
    let mut known = cache().lock().unwrap_or_else(|e| e.into_inner());
    let fresh = read_keychain_item(account);
    known.insert(account.to_string(), fresh);
    Ok(())
}

/// Remove a stored secret (nothing there is fine).
pub fn delete(account: &str) {
    #[cfg(test)]
    {
        store().lock().unwrap().remove(account);
    }
    #[cfg(not(test))]
    {
        if let Ok(e) = keyring::Entry::new(SERVICE, account) {
            let _ = e.delete_credential();
        }
        cache().lock().unwrap_or_else(|e| e.into_inner()).insert(account.to_string(), Lookup::Absent);
    }
}

/// Store a secret; an empty one removes it. Succeeds only when the keychain
/// gives the value back afterwards.
pub fn set(account: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        delete(account);
        return Ok(());
    }
    write(account, value)?;
    verify(value, lookup(account))
}

/// A write counts only when the same value can be read again. Pure.
pub fn verify(written: &str, read_back: Lookup) -> Result<(), String> {
    match read_back {
        Lookup::Found(v) if v == written => Ok(()),
        Lookup::Found(_) => Err("keychain.mismatch".into()),
        Lookup::Absent => Err("keychain.not_stored".into()),
        Lookup::Failed(why) => Err(format!("keychain: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_survives_between_calls_and_can_be_removed() {
        let a = "test-secrets-roundtrip";
        delete(a);
        assert_eq!(lookup(a), Lookup::Absent);
        assert_eq!(get(a), None);
        set(a, "geheim").unwrap();
        assert_eq!(get(a).as_deref(), Some("geheim"));
        set(a, "").unwrap();
        assert_eq!(lookup(a), Lookup::Absent);
    }

    #[test]
    fn a_blank_secret_is_none() {
        let a = "test-secrets-blank";
        set(a, "   ").unwrap();
        assert_eq!(get(a), None);
        delete(a);
    }

    #[test]
    fn a_write_that_cannot_be_read_back_is_an_error() {
        assert_eq!(verify("x", Lookup::Found("x".into())), Ok(()));
        // The mock store of a keyring without a platform feature: nothing there.
        assert_eq!(verify("x", Lookup::Absent), Err("keychain.not_stored".into()));
        assert_eq!(verify("x", Lookup::Found("y".into())), Err("keychain.mismatch".into()));
        assert!(verify("x", Lookup::Failed("locked".into())).unwrap_err().contains("locked"));
    }

    /// The manifest must keep the platform stores switched on — without them
    /// every secret silently lives in memory only.
    #[test]
    fn the_keyring_is_built_with_the_platform_stores() {
        let manifest = include_str!("../../../Cargo.toml");
        let line = manifest.lines().find(|l| l.trim_start().starts_with("keyring")).expect("keyring dependency");
        for feature in ["apple-native", "windows-native"] {
            assert!(line.contains(feature), "keyring needs the `{feature}` feature: {line}");
        }
    }
}
