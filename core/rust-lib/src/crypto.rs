//! At-rest encryption for sensitive content fields stored in SQLite.
//!
//! ## Threat model
//!
//! The previous "limitation" the user wanted to close: anyone with read
//! access to the user's profile (other apps running as the same user, a
//! stolen drive image, an accidental backup, …) could open
//! `~/Library/Application Support/InspectorRust/history.db` and read every
//! password, token, and snippet body in plaintext.
//!
//! With this module wired in, those fields are encrypted with AES-256-GCM.
//! The key lives in the OS keychain (macOS Keychain / Windows Credential
//! Manager / Linux Secret Service) so it survives reboots and is bound to
//! the logged-in user. If the keychain is unavailable (rare — locked
//! keychain, missing keychain access, or first launch on Linux without
//! a Secret Service implementation), we fall back to a 0600 keyfile in
//! the data dir. That fallback is strictly worse — file-system access
//! gets the attacker the key too — but it keeps the app usable instead
//! of crashing.
//!
//! ## Storage format
//!
//! Encrypted strings are stored as TEXT with a marker prefix:
//!
//! ```text
//! "v1:" + base64( 12-byte random nonce || aes-gcm ciphertext+tag )
//! ```
//!
//! `decrypt` is permissive: any string that does **not** start with
//! `v1:` is treated as legacy plaintext and returned as-is. That's the
//! migration hook — see [`migrate_table`].
//!
//! ## What is and isn't encrypted
//!
//! Encrypted: `entries.content_text`, `entries.content_data`,
//! `snippets.body`, `notes.content_text`, `notes.content_data`.
//!
//! NOT encrypted: timestamps, IDs, content-type tags, hashes (used for
//! dedup), abbreviations, titles, categories. None of those reveal the
//! actual clipboard content.

use std::path::Path;
use std::sync::OnceLock;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;

const PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

const KEYRING_SERVICE: &str = "io.celox.inspector-rust";
const KEYRING_USER: &str = "history-db-key-v1";
const KEYFILE_NAME: &str = ".dbkey";

/// AES-256-GCM cipher with a 32-byte key. Wrapped so callers can't
/// accidentally serialize the key.
pub struct Cipher {
    aead: Aes256Gcm,
}

impl Cipher {
    fn new(key_bytes: &[u8; KEY_LEN]) -> Self {
        let key = Key::<Aes256Gcm>::from_slice(key_bytes);
        Cipher {
            aead: Aes256Gcm::new(key),
        }
    }

    fn encrypt(&self, plain: &[u8]) -> Result<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .aead
            .encrypt(nonce, plain)
            .map_err(|_| anyhow!("AES-GCM encrypt failed"))?;
        let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(format!("{PREFIX}{}", B64.encode(combined)))
    }

    fn decrypt(&self, value: &str) -> Result<String> {
        // Legacy plaintext (or any string that wasn't encrypted by us).
        if !value.starts_with(PREFIX) {
            return Ok(value.to_string());
        }
        let combined = B64
            .decode(&value.as_bytes()[PREFIX.len()..])
            .context("invalid base64 in encrypted value")?;
        if combined.len() < NONCE_LEN {
            return Err(anyhow!("encrypted value too short"));
        }
        let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plain = self
            .aead
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow!("AES-GCM decrypt failed (wrong key or tampered data)"))?;
        String::from_utf8(plain).context("decrypted payload was not valid UTF-8")
    }
}

static CIPHER: OnceLock<Cipher> = OnceLock::new();

/// Initialize the global cipher. Must be called once, early in app
/// startup, before any DB read or write. Idempotent — second call is a
/// no-op (subsequent inits with a different key would silently produce
/// undecryptable data, so we ignore them).
///
/// Tries OS keychain first (`io.celox.inspector-rust` / `history-db-key-v1`).
/// Falls back to a 0600 keyfile under `data_dir`.
pub fn init(data_dir: &Path) -> Result<()> {
    if CIPHER.get().is_some() {
        return Ok(());
    }
    let key = load_or_create_key(data_dir)?;
    let _ = CIPHER.set(Cipher::new(&key));
    Ok(())
}

/// Encrypt a string for storage. If crypto isn't initialized, returns
/// the plaintext unchanged — this matters for tests that build
/// in-memory DBs without going through the app's setup path. In
/// production, [`init`] must have run before any DB call, so the
/// passthrough path is unreachable.
pub fn encrypt(plain: &str) -> String {
    match CIPHER.get() {
        Some(c) => c
            .encrypt(plain.as_bytes())
            .unwrap_or_else(|e| {
                tracing::warn!("encrypt failed, storing plaintext: {e:#}");
                plain.to_string()
            }),
        None => plain.to_string(),
    }
}

/// Decrypt a value read from the DB. Permissive: legacy plaintext (no
/// `v1:` prefix) is returned unchanged, so existing rows continue to
/// work until [`migrate_table`] re-encrypts them.
pub fn decrypt(value: &str) -> String {
    match CIPHER.get() {
        Some(c) => match c.decrypt(value) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("decrypt failed, returning raw: {e:#}");
                value.to_string()
            }
        },
        None => value.to_string(),
    }
}

/// Walk a TEXT column in `table` and re-encrypt every row that doesn't
/// already start with the `v1:` prefix. Idempotent — already-encrypted
/// rows are skipped. Run from the same connection pool as everything
/// else so we share the WAL.
pub fn migrate_table(
    conn: &rusqlite::Connection,
    table: &str,
    columns: &[&str],
) -> Result<usize> {
    if CIPHER.get().is_none() {
        return Ok(0);
    }
    let id_select = format!("SELECT id FROM {table}");
    let mut stmt = conn.prepare(&id_select)?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut migrated = 0usize;
    for id in ids {
        for col in columns {
            let select_q = format!("SELECT {col} FROM {table} WHERE id = ?1");
            let raw: Option<String> = conn
                .query_row(&select_q, [id], |r| r.get::<_, Option<String>>(0))?;
            let Some(raw) = raw else { continue };
            if raw.starts_with(PREFIX) || raw.is_empty() {
                continue;
            }
            let encrypted = encrypt(&raw);
            let update_q = format!("UPDATE {table} SET {col} = ?1 WHERE id = ?2");
            conn.execute(&update_q, rusqlite::params![encrypted, id])?;
            migrated += 1;
        }
    }
    Ok(migrated)
}

/// Try the OS keychain first, then a keyfile in `data_dir`. Generates a
/// fresh key on first run and stores it in the keychain (and writes a
/// keyfile copy as a fallback so a future keychain-unavailable session
/// can still open the DB).
/// What a key store answered. The distinction between [`Absent`](KeyLookup::Absent)
/// and [`Failed`](KeyLookup::Failed) is the whole point: minting a fresh key is
/// only ever correct when the store genuinely holds nothing.
enum KeyLookup {
    Found(Box<[u8; KEY_LEN]>),
    /// The store works and definitely has no key (first run on this machine).
    Absent,
    /// We could not find out — access denied, locked keychain, I/O error, or a
    /// value that is present but unreadable/corrupt.
    Failed(String),
}

/// Resolve the at-rest key.
///
/// ⚠️ SAFETY-CRITICAL (fixed 2026-08-16). This function is one wrong `?` away
/// from destroying every encrypted row the user owns. It previously treated
/// "the keychain said no entry" and "the keychain read FAILED" as the same
/// `None` — so a denied keychain prompt (routine after the app is re-signed)
/// plus an unreadable keyfile made it mint a brand-new key and overwrite the
/// last copy of the real one. Clipboard history, snippet bodies, notes and
/// **every TOTP secret** would then be undecryptable forever, silently: the
/// permissive `decrypt` just hands back the raw `v1:…` blob.
///
/// The rule now: **mint only when BOTH stores report genuine absence.** Any
/// read failure aborts with an error — the caller refuses to start rather than
/// replace a key it could not read.
fn load_or_create_key(data_dir: &Path) -> Result<[u8; KEY_LEN]> {
    let from_chain = read_keychain();
    if let KeyLookup::Found(k) = from_chain {
        // Keep the keyfile in sync so the fallback always works.
        let _ = write_keyfile(data_dir, &k);
        return Ok(*k);
    }

    let from_file = read_keyfile(data_dir);
    if let KeyLookup::Found(k) = from_file {
        let _ = write_keychain(&k);
        return Ok(*k);
    }

    // Neither store produced a key. Minting is safe ONLY if both are certain
    // they never had one; otherwise we would be overwriting a key that exists
    // but is momentarily unreadable.
    for (store, res) in [("keychain", &from_chain), ("keyfile", &from_file)] {
        if let KeyLookup::Failed(why) = res {
            return Err(anyhow!(
                "refusing to create a new encryption key: the {store} could not be read ({why}). \
                 A key may already exist — replacing it would make all encrypted data \
                 (clipboard history, snippets, notes, 2FA secrets) permanently unreadable. \
                 Grant access (or restore the key) and start again."
            ));
        }
    }

    // First run on this machine — mint a fresh key and persist it.
    let mut key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);
    if let Err(e) = write_keychain(&key) {
        tracing::warn!("could not store key in OS keychain: {e:#} — relying on keyfile");
    }
    write_keyfile(data_dir, &key).context("could not write keyfile fallback")?;
    Ok(key)
}

fn read_keychain() -> KeyLookup {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e,
        Err(e) => return KeyLookup::Failed(format!("entry: {e}")),
    };
    let secret = match entry.get_password() {
        Ok(s) => s,
        // The ONLY answer that means "there is nothing here".
        Err(keyring::Error::NoEntry) => return KeyLookup::Absent,
        Err(e) => return KeyLookup::Failed(e.to_string()),
    };
    decode_key(secret.as_bytes(), "keychain value")
}

/// Decode a stored (base64) key. A value that exists but does not decode is
/// `Failed`, never `Absent` — it is evidence a key WAS set here.
fn decode_key(raw: &[u8], what: &str) -> KeyLookup {
    let decoded = match B64.decode(raw) {
        Ok(d) => d,
        Err(e) => return KeyLookup::Failed(format!("{what} is not valid base64: {e}")),
    };
    let len = decoded.len();
    match <[u8; KEY_LEN]>::try_from(decoded) {
        Ok(k) => KeyLookup::Found(Box::new(k)),
        Err(_) => KeyLookup::Failed(format!("{what} is {len} bytes, expected {KEY_LEN}")),
    }
}

fn write_keychain(key: &[u8; KEY_LEN]) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .context("keyring entry create failed")?;
    let encoded = B64.encode(key);
    entry
        .set_password(&encoded)
        .context("keyring set_password failed")?;
    Ok(())
}

fn keyfile_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(KEYFILE_NAME)
}

fn read_keyfile(data_dir: &Path) -> KeyLookup {
    let path = keyfile_path(data_dir);
    match std::fs::read(&path) {
        Ok(bytes) => decode_key(&bytes, "keyfile"),
        // Not there = genuinely absent. Anything else (permissions, I/O) is a
        // failure: the file may exist and hold the only copy of the key.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeyLookup::Absent,
        Err(e) => KeyLookup::Failed(format!("{}: {e}", path.display())),
    }
}

fn write_keyfile(data_dir: &Path, key: &[u8; KEY_LEN]) -> Result<()> {
    let path = keyfile_path(data_dir);
    let encoded = B64.encode(key);
    std::fs::write(&path, encoded.as_bytes())
        .with_context(|| format!("write keyfile {}", path.display()))?;
    // Best-effort restrictive permissions on Unix-likes.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Direct cipher roundtrip — bypasses the global state so tests
    /// don't fight over the OnceLock.
    #[test]
    fn cipher_roundtrip() {
        let mut key = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut key);
        let c = Cipher::new(&key);
        let plain = "hello, secrets!";
        let enc = c.encrypt(plain.as_bytes()).unwrap();
        assert!(enc.starts_with(PREFIX));
        assert_ne!(enc, plain);
        let dec = c.decrypt(&enc).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn cipher_passes_through_legacy_plaintext() {
        let key = [42u8; KEY_LEN];
        let c = Cipher::new(&key);
        let dec = c.decrypt("not encrypted").unwrap();
        assert_eq!(dec, "not encrypted");
    }

    #[test]
    fn cipher_handles_empty_string() {
        let key = [7u8; KEY_LEN];
        let c = Cipher::new(&key);
        let enc = c.encrypt(b"").unwrap();
        let dec = c.decrypt(&enc).unwrap();
        assert_eq!(dec, "");
    }

    #[test]
    fn cipher_each_encrypt_uses_fresh_nonce() {
        let key = [1u8; KEY_LEN];
        let c = Cipher::new(&key);
        let enc1 = c.encrypt(b"same").unwrap();
        let enc2 = c.encrypt(b"same").unwrap();
        assert_ne!(enc1, enc2, "nonce must be random per encryption");
    }

    #[test]
    fn cipher_rejects_tampered_ciphertext() {
        let key = [9u8; KEY_LEN];
        let c = Cipher::new(&key);
        let enc = c.encrypt(b"keep me safe").unwrap();
        // Flip a byte in the base64 payload.
        let mut bytes = enc.into_bytes();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(c.decrypt(&tampered).is_err());
    }

    #[test]
    fn cipher_rejects_wrong_key() {
        let key1 = [1u8; KEY_LEN];
        let key2 = [2u8; KEY_LEN];
        let c1 = Cipher::new(&key1);
        let c2 = Cipher::new(&key2);
        let enc = c1.encrypt(b"hello").unwrap();
        assert!(c2.decrypt(&enc).is_err());
    }

    #[test]
    fn cipher_handles_unicode_payloads() {
        let key = [3u8; KEY_LEN];
        let c = Cipher::new(&key);
        // Mixed scripts + emoji + supplementary plane chars
        let plain = "Hallo 世界 🌍🦀 Привет اَلْعَرَبِيَّةُ 𝕳𝖊𝖑𝖑𝖔";
        let enc = c.encrypt(plain.as_bytes()).unwrap();
        let dec = c.decrypt(&enc).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn cipher_handles_large_payload() {
        let key = [11u8; KEY_LEN];
        let c = Cipher::new(&key);
        // 1 MB of pseudo-random bytes simulating a base64'd image payload.
        let mut payload = vec![0u8; 1024 * 1024];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i % 251) as u8; // avoid all-zeroes
        }
        let plain = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &payload);
        let enc = c.encrypt(plain.as_bytes()).unwrap();
        let dec = c.decrypt(&enc).unwrap();
        assert_eq!(dec.len(), plain.len());
        assert_eq!(dec, plain);
    }

    #[test]
    fn cipher_handles_strings_with_embedded_nuls_and_newlines() {
        let key = [13u8; KEY_LEN];
        let c = Cipher::new(&key);
        let plain = "line 1\n\0nul-byte\nline 3\twith\ttabs";
        let enc = c.encrypt(plain.as_bytes()).unwrap();
        let dec = c.decrypt(&enc).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn cipher_output_always_carries_the_prefix() {
        let key = [17u8; KEY_LEN];
        let c = Cipher::new(&key);
        for plain in ["", "a", "hello world", "🦀"] {
            let enc = c.encrypt(plain.as_bytes()).unwrap();
            assert!(
                enc.starts_with(PREFIX),
                "encrypted output for {plain:?} must start with {PREFIX:?}; got {enc:?}",
            );
        }
    }

    #[test]
    fn cipher_decrypt_rejects_truncated_ciphertext() {
        let key = [19u8; KEY_LEN];
        let c = Cipher::new(&key);
        let enc = c.encrypt(b"some content").unwrap();
        // Truncate to less than the nonce + tag would need.
        let chopped = &enc[..PREFIX.len() + 4];
        assert!(c.decrypt(chopped).is_err());
    }

    #[test]
    fn cipher_decrypt_rejects_garbled_base64() {
        let key = [21u8; KEY_LEN];
        let c = Cipher::new(&key);
        let garbage = format!("{PREFIX}!!!not-valid-base64!!!");
        assert!(c.decrypt(&garbage).is_err());
    }

    #[test]
    fn cipher_handles_long_runs_of_a_single_byte() {
        // Catches modes that would leak repetition (it's GCM so it doesn't,
        // but make it explicit).
        let key = [23u8; KEY_LEN];
        let c = Cipher::new(&key);
        let plain = "A".repeat(8192);
        let enc1 = c.encrypt(plain.as_bytes()).unwrap();
        let enc2 = c.encrypt(plain.as_bytes()).unwrap();
        assert_ne!(enc1, enc2);
        let dec = c.decrypt(&enc1).unwrap();
        assert_eq!(dec.len(), plain.len());
        assert_eq!(dec, plain);
    }

    #[test]
    fn cipher_key_length_constant_is_aes_256() {
        // AES-256 ⇒ 32-byte key; making this explicit so a future hand-edit
        // doesn't silently downgrade us to AES-128.
        assert_eq!(KEY_LEN, 32);
    }

    #[test]
    fn cipher_encrypted_output_is_strictly_longer_than_input() {
        // Sanity: GCM adds a 16-byte tag + 12-byte nonce + base64 expansion +
        // the prefix. For any non-trivial input the encrypted form is bigger.
        let key = [25u8; KEY_LEN];
        let c = Cipher::new(&key);
        let plain = "hello";
        let enc = c.encrypt(plain.as_bytes()).unwrap();
        assert!(
            enc.len() > plain.len() + PREFIX.len(),
            "encrypted output ({} bytes) should be bigger than plain+prefix ({} bytes)",
            enc.len(),
            plain.len() + PREFIX.len(),
        );
    }

    #[test]
    fn migrate_table_is_a_noop_without_a_key() {
        // Safety property: on an install where the cipher was never initialised
        // (CIPHER unset — the state of the whole test process), migrate_table
        // must leave every row byte-for-byte untouched and report 0 migrations,
        // rather than corrupt plaintext it can't round-trip. This guards the
        // `CIPHER.get().is_none()` early return.
        assert!(
            CIPHER.get().is_none(),
            "test process must not have an initialised global cipher",
        );
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, secret TEXT, other TEXT);
             INSERT INTO t (id, secret, other) VALUES (1, 'plain-1', 'x'), (2, 'plain-2', NULL);",
        )
        .unwrap();

        let migrated = migrate_table(&conn, "t", &["secret", "other"]).unwrap();
        assert_eq!(migrated, 0, "no key ⇒ nothing migrated");

        // Rows are unchanged (no accidental mutation).
        let s1: String = conn
            .query_row("SELECT secret FROM t WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(s1, "plain-1");
        let s2: Option<String> = conn
            .query_row("SELECT other FROM t WHERE id = 2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(s2, None);
    }

    // ── Key resolution: the "never mint over an existing key" guarantee ──────
    //
    // These pin the 2026-08-16 fix. The old code collapsed "no entry" and
    // "read failed" into one `None`, so a denied keychain prompt plus an
    // unreadable keyfile minted a NEW key and overwrote the last copy of the
    // real one — silently making all encrypted data unreadable forever.

    #[test]
    fn a_key_that_exists_but_does_not_decode_is_a_failure_not_an_absence() {
        // Evidence a key WAS stored here. Reporting `Absent` would licence
        // minting a replacement — the exact data-loss path.
        for bad in [
            &b"not base64 !!"[..],
            &b""[..],                       // empty file
            B64.encode([0u8; 16]).as_bytes(), // right encoding, wrong length
        ] {
            assert!(
                matches!(decode_key(bad, "keyfile"), KeyLookup::Failed(_)),
                "unreadable stored key must be Failed, never Absent",
            );
        }
        // A well-formed key of the right length is the only Found case.
        assert!(matches!(
            decode_key(B64.encode([7u8; KEY_LEN]).as_bytes(), "keyfile"),
            KeyLookup::Found(k) if *k == [7u8; KEY_LEN]
        ));
    }

    #[test]
    fn a_missing_keyfile_is_absent_but_an_unreadable_one_is_a_failure() {
        let dir = std::env::temp_dir().join(format!("ir-keytest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(keyfile_path(&dir));
        // Genuinely nothing there → first-run, minting is allowed.
        assert!(matches!(read_keyfile(&dir), KeyLookup::Absent));
        // Present but corrupt → must NOT be mistaken for a first run.
        std::fs::write(keyfile_path(&dir), b"garbage").unwrap();
        assert!(matches!(read_keyfile(&dir), KeyLookup::Failed(_)));
        // Round-trip of a real key.
        write_keyfile(&dir, &[3u8; KEY_LEN]).unwrap();
        assert!(matches!(read_keyfile(&dir), KeyLookup::Found(k) if *k == [3u8; KEY_LEN]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_create_refuses_to_replace_a_key_it_could_not_read() {
        // THE regression: an existing-but-unreadable keyfile must abort, and —
        // the load-bearing half — must be left byte-for-byte intact so the
        // user can still recover it.
        let dir = std::env::temp_dir().join(format!("ir-keyfail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let corrupt = b"this is not a key";
        std::fs::write(keyfile_path(&dir), corrupt).unwrap();

        let err = load_or_create_key(&dir).unwrap_err().to_string();
        assert!(err.contains("refusing to create a new encryption key"), "got: {err}");
        assert_eq!(
            std::fs::read(keyfile_path(&dir)).unwrap(),
            corrupt,
            "the unreadable key must survive the failed start — it may be recoverable",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
