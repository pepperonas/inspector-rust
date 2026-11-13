//! The app's secrets (backup password, device-sync passphrase, GitHub tokens, AI keys), in
//! one place.
//!
//! **Where they live (v0.262.0, user decision):** in `secrets.json` in the data folder
//! (0600), every value AES-256-GCM-encrypted with the database key — the same protection
//! the 2FA secrets and the history have. No keychain item, so macOS never asks for the login
//! password: a self-signed build counts as a new app for the keychain after every update,
//! and each read of an item raised a password dialog (which also switches Secure Input on
//! and locks a Logitech mouse meanwhile). The price: another program running as the same
//! user can read them without a prompt, as it already could the 2FA secrets.
//!
//! The ONE keychain account left is the database key's copy ([`KEYCHAIN_ONLY`]) — it cannot
//! be encrypted with itself; `crypto` reads the keyfile first and the keychain only as the
//! fallback for a lost keyfile.
//!
//! A secret still in the keychain from an older version is taken over on its first read
//! (one last dialog) and removed there.
//!
//! ⚠️ The `keyring` crate stores NOTHING unless a platform feature is enabled
//! (`apple-native`, `windows-native` in the workspace manifest); [`set`] reads back what it
//! wrote and fails when it is not there.
//!
//! Tests never touch the real keychain or the real file: under `cfg(test)` the public API
//! is a map in the process; the file store is tested through [`FileStore`] in a temp folder.

#[cfg_attr(test, allow(dead_code))]
pub const SERVICE: &str = "io.celox.inspector-rust";

/// The one account that stays in the OS keychain: the database key's copy (the key that
/// encrypts the file store cannot be kept in it).
pub const KEYCHAIN_ONLY: &str = "history-db-key-v1";
pub const STORE_FILE: &str = "secrets.json";

#[cfg(not(test))]
static STORE_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
#[cfg(not(test))]
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tells the store where the data folder is. Call once, right after `crypto::init`.
pub fn init(data_dir: &std::path::Path) {
    #[cfg(not(test))]
    {
        let _ = STORE_PATH.set(data_dir.join(STORE_FILE));
    }
    #[cfg(test)]
    {
        let _ = data_dir;
    }
}

/// The secrets file: account → encrypted value, or `null` for "known to be absent".
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: std::collections::BTreeMap<String, Option<String>>,
}

#[cfg_attr(test, allow(dead_code))]
impl FileStore {
    /// A missing file is an empty store; an unreadable or broken one is an error (never
    /// treated as empty — the next save would wipe what is in it).
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("secret store unreadable: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("secret store unreadable: {e}")),
        }
    }

    /// Temp file (0600) + rename: the file is only ever whole.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let mut out = self.clone();
        out.version = 1;
        let bytes = serde_json::to_vec_pretty(&out).map_err(|e| e.to_string())?;
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let res = (|| -> std::io::Result<()> {
            let _ = std::fs::remove_file(&tmp);
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            std::io::Write::write_all(&mut f, &bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        res.map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("secret store not written: {e}")
        })
    }

    /// What the file says about `account`; `None` = it has never heard of it.
    pub fn lookup(&self, account: &str, decrypt: impl Fn(&str) -> Option<String>) -> Option<Lookup> {
        Some(match self.items.get(account)? {
            None => Lookup::Absent,
            Some(enc) => match decrypt(enc) {
                Some(v) => Lookup::Found(v),
                None => Lookup::Failed("stored secret does not decrypt".into()),
            },
        })
    }

    /// Stores a value (`None` = known to be absent). Refuses when it cannot be encrypted —
    /// a secret is never written in the clear.
    pub fn put(&mut self, account: &str, value: Option<&str>, encrypt: impl Fn(&str) -> Option<String>) -> Result<(), String> {
        let stored = match value {
            None => None,
            Some(v) => Some(encrypt(v).ok_or("secret could not be encrypted")?),
        };
        self.items.insert(account.to_string(), stored);
        Ok(())
    }

    /// Takes over what the keychain said about an account the file does not know: found →
    /// stored here (and the keychain copy can go: the `bool`), absent → remembered as
    /// absent, unreadable → nothing recorded, so it is tried again on the next run.
    pub fn take_over(&mut self, account: &str, keychain: Lookup, encrypt: impl Fn(&str) -> Option<String>) -> (Lookup, bool) {
        match keychain {
            Lookup::Found(v) => match self.put(account, Some(&v), encrypt) {
                Ok(()) => (Lookup::Found(v), true),
                // Not encryptable right now: use it, keep it where it is.
                Err(_) => (Lookup::Found(v), false),
            },
            Lookup::Absent => {
                self.items.insert(account.to_string(), None);
                (Lookup::Absent, false)
            }
            Lookup::Failed(why) => (Lookup::Failed(why), false),
        }
    }
}

/// What the store says about an account.
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
    let fresh = if account == KEYCHAIN_ONLY { read_keychain_item(account) } else { read_stored(account) };
    known.insert(account.to_string(), fresh.clone());
    fresh
}

/// A secret from the file; one the file does not know yet is looked for in the keychain
/// once (older versions kept it there), taken over and removed there.
#[cfg(not(test))]
fn read_stored(account: &str) -> Lookup {
    let Some(path) = STORE_PATH.get() else { return Lookup::Failed("secret store not ready".into()) };
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut store = match FileStore::load(path) {
        Ok(s) => s,
        Err(e) => return Lookup::Failed(e),
    };
    if let Some(known) = store.lookup(account, crate::crypto::decrypt_strict) {
        return known;
    }
    let old = read_keychain_item(account);
    let (answer, cleanup) = store.take_over(account, old, crate::crypto::encrypt_strict);
    if !matches!(answer, Lookup::Failed(_)) {
        if let Err(e) = store.save(path) {
            tracing::warn!("secrets: could not write the secret store: {e}");
            return answer;
        }
        if cleanup {
            if let Ok(e) = keyring::Entry::new(SERVICE, account) {
                let _ = e.delete_credential();
            }
            tracing::info!("secrets: took {account} over from the keychain");
        }
    }
    answer
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
    let fresh = if account == KEYCHAIN_ONLY {
        keyring::Entry::new(SERVICE, account).and_then(|e| e.set_password(value)).map_err(|e| e.to_string())?;
        read_keychain_item(account)
    } else {
        let path = STORE_PATH.get().ok_or("secret store not ready")?;
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = FileStore::load(path)?;
        store.put(account, Some(value), crate::crypto::encrypt_strict)?;
        store.save(path)?;
        // Re-read from the disk (the write check in `set`).
        FileStore::load(path)?.lookup(account, crate::crypto::decrypt_strict).unwrap_or(Lookup::Absent)
    };
    // Then answer from memory.
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(account.to_string(), fresh);
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
        if account == KEYCHAIN_ONLY {
            if let Ok(e) = keyring::Entry::new(SERVICE, account) {
                let _ = e.delete_credential();
            }
        } else if let Some(path) = STORE_PATH.get() {
            let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            // "Known to be absent" — so an old keychain copy is not taken over again.
            let saved = FileStore::load(path).and_then(|mut s| {
                s.put(account, None, crate::crypto::encrypt_strict)?;
                s.save(path)
            });
            if let Err(e) = saved {
                tracing::warn!("secrets: could not remove {account}: {e}");
            }
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

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ir-secrets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join(STORE_FILE)
    }
    // A stand-in cipher: reversible, and visibly not the plain text.
    fn enc(v: &str) -> Option<String> {
        Some(format!("v1:{}", v.chars().rev().collect::<String>()))
    }
    fn dec(v: &str) -> Option<String> {
        v.strip_prefix("v1:").map(|x| x.chars().rev().collect())
    }

    #[test]
    fn the_file_store_keeps_secrets_encrypted_and_private() {
        let path = tmp("roundtrip");
        let mut s = FileStore::load(&path).unwrap();
        assert_eq!(s.lookup("a", dec), None, "never heard of it");
        s.put("a", Some("hunter2"), enc).unwrap();
        s.put("gone", None, enc).unwrap();
        s.save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("hunter2"), "{raw}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let back = FileStore::load(&path).unwrap();
        assert_eq!(back.lookup("a", dec), Some(Lookup::Found("hunter2".into())));
        assert_eq!(back.lookup("gone", dec), Some(Lookup::Absent));
        // A value that no longer decrypts is a failure, never "absent".
        assert!(matches!(back.lookup("a", |_| None), Some(Lookup::Failed(_))));
    }

    #[test]
    fn a_secret_is_never_written_in_the_clear() {
        let mut s = FileStore::default();
        assert!(s.put("a", Some("x"), |_| None).is_err());
        assert_eq!(s.lookup("a", dec), None);
    }

    #[test]
    fn a_broken_store_is_an_error_not_an_empty_one() {
        let path = tmp("broken");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(FileStore::load(&path).is_err());
    }

    #[test]
    fn an_old_keychain_secret_is_taken_over_once() {
        let mut s = FileStore::default();
        // Found: stored here, the keychain copy may go.
        assert_eq!(s.take_over("pw", Lookup::Found("alt".into()), enc), (Lookup::Found("alt".into()), true));
        assert_eq!(s.lookup("pw", dec), Some(Lookup::Found("alt".into())));
        // Absent: remembered, so the keychain is not asked again.
        assert_eq!(s.take_over("none", Lookup::Absent, enc), (Lookup::Absent, false));
        assert_eq!(s.lookup("none", dec), Some(Lookup::Absent));
        // Unreadable (dialog denied, keychain locked): nothing recorded — tried again next run.
        assert!(matches!(s.take_over("later", Lookup::Failed("denied".into()), enc).0, Lookup::Failed(_)));
        assert_eq!(s.lookup("later", dec), None);
        // Found but not encryptable: used, and it stays in the keychain.
        assert_eq!(s.take_over("raw", Lookup::Found("v".into()), |_| None), (Lookup::Found("v".into()), false));
        assert_eq!(s.lookup("raw", dec), None);
    }

    #[test]
    fn only_the_database_key_stays_in_the_keychain() {
        let src = include_str!("crypto.rs");
        assert!(src.contains(&format!("const KEYRING_USER: &str = \"{KEYCHAIN_ONLY}\";")));
    }

}
