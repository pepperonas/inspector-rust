//! TOTP (Time-Based One-Time Password, RFC 6238) entry storage.
//!
//! ## Encryption
//!
//! TOTP secrets are the long-term root key for the user's 2FA — leak
//! one and the attacker has continuous access until rotation. We
//! therefore **never** persist them in plaintext. Each secret is
//! encrypted before DB write using the existing [`crate::crypto`]
//! AES-GCM cipher (key in macOS Keychain via `keyring`), and
//! decrypted only on-demand for code generation.
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS totp_entries (
//!     id            INTEGER PRIMARY KEY AUTOINCREMENT,
//!     issuer        TEXT NOT NULL,    -- "Amazon", "GitHub", …
//!     account       TEXT NOT NULL,    -- "user@example.com", "@handle"
//!     secret_enc    TEXT NOT NULL,    -- crypto::encrypt(base32 secret)
//!     digits        INTEGER NOT NULL DEFAULT 6,
//!     period        INTEGER NOT NULL DEFAULT 30,
//!     algorithm     TEXT NOT NULL DEFAULT 'SHA1',  -- SHA1 | SHA256 | SHA512
//!     created_at    INTEGER NOT NULL                -- unix seconds
//! );
//! CREATE INDEX IF NOT EXISTS idx_totp_issuer ON totp_entries (LOWER(issuer));
//! ```
//!
//! Index on lowercased issuer drives the `otp <issuer>` autocomplete
//! fuzzy match in the popup.

use crate::crypto;
use crate::db::DbHandle;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// One TOTP entry as it appears to the frontend — secret is NOT
/// included by design. The frontend asks for the current code via a
/// separate IPC; the raw secret never crosses the IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpEntry {
    pub id: i64,
    pub issuer: String,
    pub account: String,
    pub digits: u32,
    pub period: u32,
    pub algorithm: String,
    pub created_at: i64,
}

/// Generation result for a single entry.
#[derive(Debug, Clone, Serialize)]
pub struct TotpCode {
    pub code: String,
    pub seconds_remaining: u32,
}

pub fn init_table(db: &DbHandle) -> Result<()> {
    let conn = db.lock();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS totp_entries (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            issuer      TEXT NOT NULL,
            account     TEXT NOT NULL,
            secret_enc  TEXT NOT NULL,
            digits      INTEGER NOT NULL DEFAULT 6,
            period      INTEGER NOT NULL DEFAULT 30,
            algorithm   TEXT NOT NULL DEFAULT 'SHA1',
            created_at  INTEGER NOT NULL
        )",
        [],
    )
    .context("create totp_entries table")?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_totp_issuer ON totp_entries (LOWER(issuer))",
        [],
    )
    .context("create totp_issuer index")?;
    // Lazy migration: manual sort order (v0.84.200). ALTER errors harmlessly if
    // the column already exists; seed NULLs from `id` so existing rows keep a
    // stable order until the user reorders them.
    let _ = conn.execute("ALTER TABLE totp_entries ADD COLUMN sort_order INTEGER", []);
    let _ = conn.execute(
        "UPDATE totp_entries SET sort_order = id WHERE sort_order IS NULL",
        [],
    );
    Ok(())
}

/// Insert a new entry. Secret must already be **base32-encoded** per
/// RFC 4648 (no padding) — that's the canonical format every QR-code
/// import produces. Defaults: 6 digits, 30 s, SHA1 (match all major
/// authenticator apps).
pub fn add(
    db: &DbHandle,
    issuer: &str,
    account: &str,
    secret_base32: &str,
    digits: u32,
    period: u32,
    algorithm: &str,
) -> Result<TotpEntry> {
    if issuer.trim().is_empty() {
        return Err(anyhow!("issuer cannot be empty"));
    }
    if secret_base32.trim().is_empty() {
        return Err(anyhow!("secret cannot be empty"));
    }
    // Validate that the secret is decodable base32 — fail fast at
    // insert time rather than at first code-generation attempt.
    let normalised_secret = normalise_secret(secret_base32);
    decode_base32(&normalised_secret).context("secret is not valid base32")?;

    let algorithm = normalise_algorithm(algorithm);
    let now = chrono::Utc::now().timestamp();
    let encrypted = crypto::encrypt(&normalised_secret);

    let conn = db.lock();
    conn.execute(
        "INSERT INTO totp_entries (issuer, account, secret_enc, digits, period, algorithm, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![issuer.trim(), account.trim(), encrypted, digits, period, algorithm, now],
    )?;
    let id = conn.last_insert_rowid();
    // New entries append to the end of the manual order.
    conn.execute("UPDATE totp_entries SET sort_order = ?1 WHERE id = ?1", [id])?;
    Ok(TotpEntry {
        id,
        issuer: issuer.trim().to_string(),
        account: account.trim().to_string(),
        digits,
        period,
        algorithm,
        created_at: now,
    })
}

pub fn list(db: &DbHandle) -> Result<Vec<TotpEntry>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, issuer, account, digits, period, algorithm, created_at
         FROM totp_entries ORDER BY COALESCE(sort_order, id), LOWER(issuer), LOWER(account)",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TotpEntry {
                id: row.get(0)?,
                issuer: row.get(1)?,
                account: row.get(2)?,
                digits: row.get(3)?,
                period: row.get(4)?,
                algorithm: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

pub fn delete(db: &DbHandle, id: i64) -> Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM totp_entries WHERE id = ?1", [id])?;
    Ok(())
}

/// Persist a manual reorder: `ordered_ids` top-to-bottom.
pub fn set_order(db: &DbHandle, ordered_ids: &[i64]) -> Result<()> {
    let conn = db.lock();
    for (pos, id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE totp_entries SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![pos as i64, id],
        )?;
    }
    Ok(())
}

/// Update an entry. Every field is editable; **the secret stays as-is when
/// `secret_base32` is blank** (the list never exposes the current secret, so the
/// edit form leaves it empty unless the user wants to replace it). A non-blank
/// secret is base32-validated + re-encrypted.
#[allow(clippy::too_many_arguments)]
pub fn update(
    db: &DbHandle,
    id: i64,
    issuer: &str,
    account: &str,
    secret_base32: &str,
    digits: u32,
    period: u32,
    algorithm: &str,
) -> Result<()> {
    if issuer.trim().is_empty() {
        return Err(anyhow!("issuer cannot be empty"));
    }
    let algorithm = normalise_algorithm(algorithm);
    // Validate + encrypt the new secret up front (if one was supplied).
    let new_secret_enc = if secret_base32.trim().is_empty() {
        None
    } else {
        let normalised = normalise_secret(secret_base32);
        decode_base32(&normalised).context("secret is not valid base32")?;
        Some(crypto::encrypt(&normalised))
    };
    let conn = db.lock();
    let n = match &new_secret_enc {
        Some(enc) => conn.execute(
            "UPDATE totp_entries SET issuer=?1, account=?2, secret_enc=?3, digits=?4, period=?5, algorithm=?6 WHERE id=?7",
            rusqlite::params![issuer.trim(), account.trim(), enc, digits, period, algorithm, id],
        )?,
        None => conn.execute(
            "UPDATE totp_entries SET issuer=?1, account=?2, digits=?3, period=?4, algorithm=?5 WHERE id=?6",
            rusqlite::params![issuer.trim(), account.trim(), digits, period, algorithm, id],
        )?,
    };
    if n == 0 {
        return Err(anyhow!("no entry with id {id}"));
    }
    Ok(())
}

/// A duplicate-detection key: `(lower issuer, lower account, normalised secret)`.
pub fn dedup_key(issuer: &str, account: &str, secret_base32: &str) -> (String, String, String) {
    (
        issuer.trim().to_ascii_lowercase(),
        account.trim().to_ascii_lowercase(),
        normalise_secret(secret_base32),
    )
}

/// The set of dedup keys already stored — used to skip identical entries on
/// import. Decrypts each secret (one-time cost at import).
pub fn existing_keys(db: &DbHandle) -> Result<std::collections::HashSet<(String, String, String)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT issuer, account, secret_enc FROM totp_entries")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    drop(stmt);
    drop(conn);
    Ok(rows
        .into_iter()
        .map(|(iss, acc, enc)| dedup_key(&iss, &acc, &crypto::decrypt(&enc)))
        .collect())
}

/// Remove duplicate entries (same issuer+account+secret), keeping the lowest
/// id of each group. Returns how many rows were deleted.
pub fn remove_duplicates(db: &DbHandle) -> Result<usize> {
    let conn = db.lock();
    let mut stmt =
        conn.prepare("SELECT id, issuer, account, secret_enc FROM totp_entries ORDER BY id")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    drop(stmt);
    let mut seen = std::collections::HashSet::new();
    let mut to_delete = Vec::new();
    for (id, iss, acc, enc) in rows {
        if !seen.insert(dedup_key(&iss, &acc, &crypto::decrypt(&enc))) {
            to_delete.push(id);
        }
    }
    for id in &to_delete {
        conn.execute("DELETE FROM totp_entries WHERE id = ?1", [id])?;
    }
    Ok(to_delete.len())
}

/// Delete every entry. Returns how many were removed.
pub fn delete_all(db: &DbHandle) -> Result<usize> {
    let conn = db.lock();
    Ok(conn.execute("DELETE FROM totp_entries", [])?)
}

/// Compute the current TOTP code for an entry. Decrypts the secret
/// on-demand, generates code, returns code + seconds-until-next-roll.
pub fn current_code(db: &DbHandle, id: i64) -> Result<TotpCode> {
    let (secret_enc, digits, period, algorithm) = {
        let conn = db.lock();
        let mut stmt = conn.prepare(
            "SELECT secret_enc, digits, period, algorithm FROM totp_entries WHERE id = ?1",
        )?;
        stmt.query_row([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
    };
    let secret_base32 = crypto::decrypt(&secret_enc);
    generate(&secret_base32, digits, period, &algorithm)
}

/// Compute every entry's current code in one pass. Used by the
/// management overlay so the UI doesn't fire N IPCs per render tick.
pub fn current_codes_all(db: &DbHandle) -> Result<Vec<(i64, TotpCode)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, secret_enc, digits, period, algorithm FROM totp_entries",
    )?;
    let rows: Vec<(i64, String, u32, u32, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    drop(stmt);
    drop(conn);
    let mut out = Vec::with_capacity(rows.len());
    for (id, enc, digits, period, algo) in rows {
        let secret = crypto::decrypt(&enc);
        match generate(&secret, digits, period, &algo) {
            Ok(code) => out.push((id, code)),
            Err(e) => tracing::warn!("totp[{id}] generate failed: {e:#}"),
        }
    }
    Ok(out)
}

// ── TOTP generation via totp-rs ──────────────────────────────────────

/// Generate the current TOTP code. Pulled out so the unit tests can
/// hit it with the RFC 6238 test vectors without touching the DB.
pub fn generate(
    secret_base32: &str,
    digits: u32,
    period: u32,
    algorithm: &str,
) -> Result<TotpCode> {
    let now = chrono::Utc::now().timestamp() as u64;
    generate_at(secret_base32, digits, period, algorithm, now)
}

/// Time-injected core of [`generate`] — the only difference is that `now`
/// (Unix seconds) is supplied by the caller instead of read from the clock,
/// which makes the RFC 6238 test vectors reproducible. `generate` is just
/// `generate_at(.., now())`.
pub fn generate_at(
    secret_base32: &str,
    digits: u32,
    period: u32,
    algorithm: &str,
    now: u64,
) -> Result<TotpCode> {
    use totp_rs::{Algorithm, TOTP};
    let algo = match algorithm.to_ascii_uppercase().as_str() {
        "SHA1" | "" => Algorithm::SHA1,
        "SHA256" => Algorithm::SHA256,
        "SHA512" => Algorithm::SHA512,
        other => return Err(anyhow!("unsupported TOTP algorithm: {other}")),
    };
    let secret_bytes = decode_base32(secret_base32)?;
    // `TOTP::new` rejects secrets shorter than 128 bits, but plenty of real,
    // working secrets are shorter (e.g. 80-bit / 16-char base32 from OTPManager,
    // Stripe, etc.) — Google Authenticator generates codes for them fine. Use
    // `new_unchecked` so those import cleanly + show a code instead of blank.
    let totp = TOTP::new_unchecked(algo, digits as usize, 1, period as u64, secret_bytes);
    let code = totp.generate(now);
    let seconds_remaining = (period as u64) - (now % period as u64);
    Ok(TotpCode {
        code,
        seconds_remaining: seconds_remaining as u32,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Trim whitespace and strip dashes / spaces commonly inserted in
/// pretty-printed secrets. RFC 4648 base32 doesn't allow them so
/// totp-rs would reject the raw input.
pub fn normalise_secret(input: &str) -> String {
    // `=` padding is stripped too (2026-08-16): base32 decodes identically
    // with or without it, but the dedup key compared the strings VERBATIM —
    // so a padded export (some apps) and an unpadded one (Google
    // Authenticator) of the SAME account imported as two separate entries.
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '=')
        .collect::<String>()
        .to_ascii_uppercase()
}

fn normalise_algorithm(input: &str) -> String {
    match input.to_ascii_uppercase().as_str() {
        "SHA256" => "SHA256".to_string(),
        "SHA512" => "SHA512".to_string(),
        _ => "SHA1".to_string(),
    }
}

/// Decode an RFC 4648 base32 string into raw bytes. Padding-tolerant
/// (Google Authenticator emits unpadded; some apps emit padded).
pub fn decode_base32(s: &str) -> Result<Vec<u8>> {
    let s = s.trim_end_matches('=').to_ascii_uppercase();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u32;
    let mut bit_count = 0u32;
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    for c in s.bytes() {
        let idx = alphabet
            .iter()
            .position(|&x| x == c)
            .ok_or_else(|| anyhow!("base32: invalid char {:?}", c as char))?;
        bits = (bits << 5) | (idx as u32);
        bit_count += 5;
        while bit_count >= 8 {
            bit_count -= 8;
            out.push((bits >> bit_count) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_secret_strips_whitespace_dashes_lowercase() {
        assert_eq!(normalise_secret("jbsw y3dp ehpk 3pxp"), "JBSWY3DPEHPK3PXP");
        assert_eq!(normalise_secret("JBSW-Y3DP-EHPK-3PXP"), "JBSWY3DPEHPK3PXP");
        assert_eq!(normalise_secret("  jbswy3dpehpk3pxp  "), "JBSWY3DPEHPK3PXP");
    }

    #[test]
    fn decode_base32_matches_rfc_4648_examples() {
        // RFC 4648 §10 reference values.
        assert_eq!(decode_base32("").unwrap(), b"");
        assert_eq!(decode_base32("MY").unwrap(), b"f");
        assert_eq!(decode_base32("MZXQ").unwrap(), b"fo");
        assert_eq!(decode_base32("MZXW6").unwrap(), b"foo");
        assert_eq!(decode_base32("MZXW6YQ").unwrap(), b"foob");
        assert_eq!(decode_base32("MZXW6YTB").unwrap(), b"fooba");
        assert_eq!(decode_base32("MZXW6YTBOI").unwrap(), b"foobar");
    }

    #[test]
    fn decode_base32_is_padding_tolerant() {
        assert_eq!(decode_base32("MZXW6YQ=").unwrap(), b"foob");
        assert_eq!(decode_base32("MZXW6YQ==").unwrap(), b"foob");
    }

    #[test]
    fn decode_base32_rejects_invalid_chars() {
        assert!(decode_base32("!@#$").is_err());
        assert!(decode_base32("1234567890").is_err()); // 0, 1, 8, 9 not in alphabet
    }

    #[test]
    fn generate_returns_six_digits_by_default() {
        // RFC 6238 test secret "12345678901234567890" base32-encoded.
        let r = generate("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ", 6, 30, "SHA1").unwrap();
        assert_eq!(r.code.len(), 6);
        assert!(r.code.chars().all(|c| c.is_ascii_digit()));
        assert!(r.seconds_remaining <= 30);
        assert!(r.seconds_remaining > 0);
    }

    // RFC 6238 Appendix B reference values — the canonical TOTP test vectors,
    // 8 digits, 30 s period. The per-algorithm seeds are the ASCII string
    // "1234567890…" truncated/extended to the key length, base32-encoded.
    const SEED_SHA1: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    const SEED_SHA256: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA";
    const SEED_SHA512: &str =
        "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNA";

    #[test]
    fn rfc6238_sha1_vectors() {
        for (t, expected) in [
            (59u64, "94287082"),
            (1111111109, "07081804"),
            (1111111111, "14050471"),
            (1234567890, "89005924"),
            (2000000000, "69279037"),
            (20000000000, "65353130"),
        ] {
            let r = generate_at(SEED_SHA1, 8, 30, "SHA1", t).unwrap();
            assert_eq!(r.code, expected, "SHA1 @ T={t}");
        }
    }

    #[test]
    fn rfc6238_sha256_vectors() {
        for (t, expected) in [
            (59u64, "46119246"),
            (1111111109, "68084774"),
            (1111111111, "67062674"),
            (1234567890, "91819424"),
            (2000000000, "90698825"),
            (20000000000, "77737706"),
        ] {
            let r = generate_at(SEED_SHA256, 8, 30, "SHA256", t).unwrap();
            assert_eq!(r.code, expected, "SHA256 @ T={t}");
        }
    }

    #[test]
    fn rfc6238_sha512_vectors() {
        for (t, expected) in [
            (59u64, "90693936"),
            (1111111109, "25091201"),
            (1111111111, "99943326"),
            (1234567890, "93441116"),
            (2000000000, "38618901"),
            (20000000000, "47863826"),
        ] {
            let r = generate_at(SEED_SHA512, 8, 30, "SHA512", t).unwrap();
            assert_eq!(r.code, expected, "SHA512 @ T={t}");
        }
    }

    #[test]
    fn generate_at_seconds_remaining_tracks_the_period() {
        // 999_999_995 % 30 == 5 → 5 s into the window → 25 s left.
        let r = generate_at(SEED_SHA1, 6, 30, "SHA1", 999_999_995).unwrap();
        assert_eq!(r.seconds_remaining, 25);
        // 1_000_000_020 % 30 == 0 → exactly on a boundary → a full period left.
        let r2 = generate_at(SEED_SHA1, 6, 30, "SHA1", 1_000_000_020).unwrap();
        assert_eq!(r2.seconds_remaining, 30);
    }

    #[test]
    fn generate_rejects_unknown_algorithm() {
        assert!(generate_at(SEED_SHA1, 6, 30, "MD5", 59).is_err());
    }

    #[test]
    fn generate_works_for_short_sub_128bit_secrets() {
        // TOTP::new rejects < 128-bit secrets; new_unchecked (what we use) does
        // not — these shorter secrets are valid + common (OTPManager, Stripe, …).
        // 16 base32 chars = 80 bits.
        let c = generate_at("GEZDGNBVGY3TQOJQ", 6, 30, "SHA1", 0).unwrap();
        assert_eq!(c.code.len(), 6);
        assert!(c.code.chars().all(|ch| ch.is_ascii_digit()));
        // 24 base32 chars = 120 bits.
        let c2 = generate_at("GEZDGNBVGY3TQOJQGEZDGNBV", 6, 30, "SHA1", 0).unwrap();
        assert_eq!(c2.code.len(), 6);
    }

    #[test]
    fn dedup_key_is_case_insensitive_and_normalised() {
        assert_eq!(
            dedup_key("GitHub", "  Me ", "jbsw y3dp"),
            dedup_key("github", "me", "JBSW-Y3DP"),
        );
        assert_ne!(dedup_key("A", "b", "JBSWY3DP"), dedup_key("A", "b", "MZXW6YTB"));
    }

    #[test]
    fn padded_and_unpadded_exports_of_the_same_secret_are_one_account() {
        // REGRESSION (2026-08-16): base32 decodes identically with or without
        // `=` padding, but the dedup key compared verbatim — importing a
        // padded export next to an unpadded one duplicated every account.
        assert_eq!(
            dedup_key("GitHub", "me", "MZXW6YQ=="),
            dedup_key("GitHub", "me", "MZXW6YQ"),
        );
        assert_eq!(normalise_secret("mzxw 6yq=="), "MZXW6YQ");
    }

    // ── DB layer (in-memory; crypto is a passthrough in the test process) ──

    fn mem_db() -> DbHandle {
        let db: DbHandle = std::sync::Arc::new(parking_lot::Mutex::new(
            rusqlite::Connection::open_in_memory().unwrap(),
        ));
        init_table(&db).unwrap();
        db
    }

    #[test]
    fn add_normalises_secret_and_algorithm_and_trims_fields() {
        let db = mem_db();
        let e = add(&db, "  GitHub ", " me@example.com ", "jbsw y3dp-ehpk 3pxp", 6, 30, "sha256")
            .unwrap();
        assert_eq!(e.issuer, "GitHub");
        assert_eq!(e.account, "me@example.com");
        assert_eq!(e.algorithm, "SHA256");
        // The stored secret is the normalised base32 (public view via the
        // dedup-key set — the raw secret never leaves the store otherwise).
        let keys = existing_keys(&db).unwrap();
        assert!(keys.contains(&dedup_key("github", "me@example.com", "JBSWY3DPEHPK3PXP")));
    }

    #[test]
    fn add_rejects_empty_fields_and_invalid_base32() {
        let db = mem_db();
        assert!(add(&db, "  ", "a", "JBSWY3DP", 6, 30, "SHA1").is_err());
        assert!(add(&db, "X", "a", "   ", 6, 30, "SHA1").is_err());
        // Fail fast at insert time, not at first code generation: 0/1/8/9
        // aren't in the RFC 4648 base32 alphabet.
        assert!(add(&db, "X", "a", "10189", 6, 30, "SHA1").is_err());
        assert!(list(&db).unwrap().is_empty(), "a rejected add must not persist");
    }

    #[test]
    fn update_with_blank_secret_keeps_the_current_one() {
        // THE edit-form contract: the list never exposes the secret, so the
        // form submits it blank — that must mean "keep", never "clear".
        let db = mem_db();
        let e = add(&db, "GitHub", "me", SEED_SHA1, 6, 30, "SHA1").unwrap();
        update(&db, e.id, "GitHub Inc", "me", "", 8, 30, "SHA1").unwrap();
        let keys = existing_keys(&db).unwrap();
        assert!(
            keys.contains(&dedup_key("github inc", "me", SEED_SHA1)),
            "blank secret on edit must keep the stored secret"
        );
        let rows = list(&db).unwrap();
        assert_eq!(rows[0].issuer, "GitHub Inc");
        assert_eq!(rows[0].digits, 8, "the other fields still update");
    }

    #[test]
    fn update_with_a_new_secret_replaces_it() {
        let db = mem_db();
        let e = add(&db, "GitHub", "me", SEED_SHA1, 6, 30, "SHA1").unwrap();
        update(&db, e.id, "GitHub", "me", "mzxw 6ytb", 6, 30, "SHA1").unwrap();
        let keys = existing_keys(&db).unwrap();
        assert!(keys.contains(&dedup_key("github", "me", "MZXW6YTB")));
        assert!(!keys.contains(&dedup_key("github", "me", SEED_SHA1)));
    }

    #[test]
    fn update_rejects_missing_id_empty_issuer_and_bad_secret() {
        let db = mem_db();
        let e = add(&db, "GitHub", "me", SEED_SHA1, 6, 30, "SHA1").unwrap();
        assert!(update(&db, e.id + 999, "X", "a", "", 6, 30, "SHA1").is_err());
        assert!(update(&db, e.id, "   ", "a", "", 6, 30, "SHA1").is_err());
        assert!(update(&db, e.id, "X", "a", "not base32 0189", 6, 30, "SHA1").is_err());
        // Nothing was half-applied by the rejected updates.
        assert_eq!(list(&db).unwrap()[0].issuer, "GitHub");
    }

    #[test]
    fn set_order_drives_the_list_and_new_entries_append() {
        let db = mem_db();
        let a = add(&db, "A", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        let b = add(&db, "B", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        let c = add(&db, "C", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        set_order(&db, &[c.id, a.id, b.id]).unwrap();
        let ids: Vec<i64> = list(&db).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![c.id, a.id, b.id]);
        // A new entry lands at the END of the manual order, not amid it.
        let d = add(&db, "D", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        let ids: Vec<i64> = list(&db).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![c.id, a.id, b.id, d.id]);
    }

    #[test]
    fn remove_duplicates_normalises_and_keeps_the_oldest() {
        let db = mem_db();
        let keep = add(&db, "GitHub", "me", "JBSWY3DP", 6, 30, "SHA1").unwrap();
        // Same entry modulo case + pretty-printed secret → duplicate.
        add(&db, "github", " ME ", "jbsw-y3dp", 6, 30, "SHA1").unwrap();
        // Same issuer+account but a DIFFERENT secret → distinct account, kept.
        let other = add(&db, "GitHub", "me", "MZXW6YTB", 6, 30, "SHA1").unwrap();
        assert_eq!(remove_duplicates(&db).unwrap(), 1);
        let ids: Vec<i64> = list(&db).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![keep.id, other.id]);
        // Idempotent — a second pass finds nothing.
        assert_eq!(remove_duplicates(&db).unwrap(), 0);
    }

    #[test]
    fn delete_all_reports_the_count() {
        let db = mem_db();
        add(&db, "A", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        add(&db, "B", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        assert_eq!(delete_all(&db).unwrap(), 2);
        assert!(list(&db).unwrap().is_empty());
        assert_eq!(delete_all(&db).unwrap(), 0);
    }

    #[test]
    fn current_codes_all_respects_each_entrys_config() {
        let db = mem_db();
        let six = add(&db, "Six", "", SEED_SHA1, 6, 30, "SHA1").unwrap();
        let eight = add(&db, "Eight", "", SEED_SHA256, 8, 30, "SHA256").unwrap();
        let codes = current_codes_all(&db).unwrap();
        assert_eq!(codes.len(), 2);
        let by_id: std::collections::HashMap<i64, &TotpCode> =
            codes.iter().map(|(id, c)| (*id, c)).collect();
        assert_eq!(by_id[&six.id].code.len(), 6);
        assert_eq!(by_id[&eight.id].code.len(), 8);
        assert!(by_id[&six.id].code.chars().all(|c| c.is_ascii_digit()));
    }
}
