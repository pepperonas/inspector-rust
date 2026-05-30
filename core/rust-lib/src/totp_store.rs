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
         FROM totp_entries ORDER BY LOWER(issuer), LOWER(account)",
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
    use totp_rs::{Algorithm, TOTP};
    let algo = match algorithm.to_ascii_uppercase().as_str() {
        "SHA1" | "" => Algorithm::SHA1,
        "SHA256" => Algorithm::SHA256,
        "SHA512" => Algorithm::SHA512,
        other => return Err(anyhow!("unsupported TOTP algorithm: {other}")),
    };
    let secret_bytes = decode_base32(secret_base32)?;
    let totp = TOTP::new(algo, digits as usize, 1, period as u64, secret_bytes)
        .map_err(|e| anyhow!("TOTP::new: {e}"))?;
    let now = chrono::Utc::now().timestamp() as u64;
    let code = totp
        .generate(now);
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
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
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
}
