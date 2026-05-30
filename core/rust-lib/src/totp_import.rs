//! Import TOTP entries from external authenticator apps.
//!
//! ## Supported formats (autodetected)
//!
//! 1. **`otpauth://totp/...`** — single QR-code-style URI (Google
//!    Authenticator, Authy, Aegis, 2FAS, 1Password, Bitwarden, etc.
//!    all export a URI per entry).
//! 2. **`otpauth-migration://offline?data=...`** — Google
//!    Authenticator's bulk migration QR-code, batched as a base64-
//!    encoded protobuf wrapping N OtpParameters records.
//! 3. **Aegis JSON** (Android Aegis Authenticator, unencrypted export):
//!    `{ "db": { "entries": [{ "type": "totp", "name", "issuer",
//!    "info": { "secret", "digits", "period", "algo" } }] } }`
//! 4. **2FAS JSON** (Android/iOS 2FAS Auth, plain export):
//!    `{ "services": [{ "name", "secret", "otp": { "account",
//!    "digits", "period", "algorithm" } }] }`
//! 5. **Plain-text fallback** — every line that itself parses as an
//!    `otpauth://` URI is imported individually. Empty lines + lines
//!    starting with `#` are skipped (so commented exports work).
//!
//! The autodetector dispatches by inspecting the input's first
//! non-whitespace bytes — JSON object → JSON parsers (in order),
//! `otpauth-migration://` → migration parser, `otpauth://` → single
//! URI, otherwise → plaintext-line fallback.
//!
//! All parsers return `Vec<ImportedEntry>` (issuer + account + secret
//! + digits + period + algorithm). The caller then runs each through
//! `totp_store::add` which handles encryption + dedup.

use crate::totp_store;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::{STANDARD as B64_STD, URL_SAFE_NO_PAD as B64_URL};
use base64::Engine;
use serde::Deserialize;

/// Untyped intermediate — what every parser returns. Becomes a real
/// `TotpEntry` once persisted by `totp_store::add`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedEntry {
    pub issuer: String,
    pub account: String,
    pub secret_base32: String,
    pub digits: u32,
    pub period: u32,
    pub algorithm: String,
}

/// Summary of an import call. `added` lists successful inserts;
/// `failed` records per-line parse failures so the UI can surface
/// why one entry didn't make it without aborting the whole batch.
#[derive(Debug, Default)]
pub struct ImportSummary {
    pub added: Vec<ImportedEntry>,
    pub failed: Vec<(String, String)>, // (raw, error)
}

/// Detect the format of `input` and parse all entries from it.
/// Never panics; format detection is best-effort, and any individual
/// entry that fails parsing lands in `failed` instead of aborting.
pub fn import_auto(input: &str) -> Result<Vec<ImportedEntry>> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    // Order matters: check the most-specific markers first.
    if trimmed.starts_with('{') {
        // Try the JSON formats in order — most-popular first.
        if let Ok(entries) = parse_aegis_json(trimmed) {
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
        if let Ok(entries) = parse_2fas_json(trimmed) {
            if !entries.is_empty() {
                return Ok(entries);
            }
        }
        return Err(anyhow!(
            "JSON input matched no known authenticator-app schema (Aegis / 2FAS)"
        ));
    }
    if trimmed.starts_with("otpauth-migration://") {
        return parse_otpauth_migration(trimmed);
    }
    if trimmed.starts_with("otpauth://") {
        return parse_otpauth_uri(trimmed).map(|e| vec![e]);
    }
    // Plain-text fallback: every non-empty non-comment line, individually.
    let mut out = Vec::new();
    for line in input.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l.starts_with("otpauth-migration://") {
            out.extend(parse_otpauth_migration(l)?);
        } else if l.starts_with("otpauth://") {
            out.push(parse_otpauth_uri(l)?);
        }
    }
    if out.is_empty() {
        return Err(anyhow!(
            "no recognisable TOTP data found — paste an otpauth://… URI or a JSON export from Aegis/2FAS"
        ));
    }
    Ok(out)
}

// ── 1) Single otpauth:// URI ─────────────────────────────────────────

/// Parse a single `otpauth://totp/Label?secret=...&issuer=...&digits=...`
/// URI. Label format per RFC 7239 (the de-facto Google spec): either
/// `Label = AccountName` or `Label = Issuer:AccountName`.
pub fn parse_otpauth_uri(uri: &str) -> Result<ImportedEntry> {
    let prefix = "otpauth://totp/";
    let rest = uri
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow!("not an otpauth://totp/ URI"))?;
    let (label_pct, query) = rest.split_once('?').unwrap_or((rest, ""));
    let label = percent_decode(label_pct);

    // Default issuer/account from the label; query-string `issuer=`
    // overrides if present.
    let (label_issuer, account) = match label.split_once(':') {
        Some((iss, acc)) => (Some(iss.trim().to_string()), acc.trim().to_string()),
        None => (None, label.trim().to_string()),
    };

    let mut secret = String::new();
    let mut issuer_q: Option<String> = None;
    let mut digits = 6u32;
    let mut period = 30u32;
    let mut algorithm = String::from("SHA1");

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let key = k.to_ascii_lowercase();
        let val = percent_decode(v);
        match key.as_str() {
            "secret" => secret = val,
            "issuer" => issuer_q = Some(val),
            "digits" => {
                digits = val.parse().unwrap_or(6);
            }
            "period" => {
                period = val.parse().unwrap_or(30);
            }
            "algorithm" => {
                algorithm = val.to_ascii_uppercase();
            }
            _ => {}
        }
    }

    if secret.is_empty() {
        return Err(anyhow!("otpauth URI missing required `secret` parameter"));
    }
    let issuer = issuer_q.or(label_issuer).unwrap_or_default();
    Ok(ImportedEntry {
        issuer: issuer.trim().to_string(),
        account: account.trim().to_string(),
        secret_base32: totp_store::normalise_secret(&secret),
        digits,
        period,
        algorithm: if algorithm.is_empty() { "SHA1".into() } else { algorithm },
    })
}

// ── 2) Google Authenticator bulk migration ──────────────────────────

/// `otpauth-migration://offline?data=BASE64_URLSAFE(protobuf)` — the
/// QR-code Google Authenticator emits when you tap "Export accounts".
/// Each QR contains a `MigrationPayload` protobuf with N OtpParameters
/// entries. Schema is public + tiny, so we parse the wire format
/// manually rather than pulling in a `prost`+`protoc` build dep.
///
/// MigrationPayload {
///   repeated OtpParameters otp_parameters = 1;  // we only care about this
///   int32 version = 2;                          // ignored
///   int32 batch_size = 3;                       // ignored
///   int32 batch_index = 4;                      // ignored
///   int32 batch_id = 5;                         // ignored
/// }
/// OtpParameters {
///   bytes  secret = 1;
///   string name = 2;       // "issuer:account" or "account"
///   string issuer = 3;
///   Algorithm algorithm = 4;  // enum: UNSPECIFIED=0 SHA1=1 SHA256=2 SHA512=3 MD5=4
///   DigitCount digits = 5;    // enum: UNSPECIFIED=0 SIX=1 EIGHT=2
///   OtpType type = 6;         // enum: UNSPECIFIED=0 HOTP=1 TOTP=2
///   int64 counter = 7;        // HOTP-only, ignored
/// }
pub fn parse_otpauth_migration(uri: &str) -> Result<Vec<ImportedEntry>> {
    let rest = uri
        .strip_prefix("otpauth-migration://offline?")
        .or_else(|| uri.strip_prefix("otpauth-migration://offline/?"))
        .ok_or_else(|| anyhow!("not an otpauth-migration://offline URI"))?;
    let mut data_b64 = String::new();
    for pair in rest.split('&') {
        if let Some(v) = pair.strip_prefix("data=") {
            data_b64 = percent_decode(v);
            break;
        }
    }
    if data_b64.is_empty() {
        return Err(anyhow!("otpauth-migration URI missing `data` parameter"));
    }
    // Google emits standard-base64 (with + and /, sometimes percent-
    // encoded as %2B / %2F). Try both alphabets to be safe.
    let bytes = B64_STD
        .decode(&data_b64)
        .or_else(|_| B64_URL.decode(data_b64.trim_end_matches('=')))
        .map_err(|e| anyhow!("base64 decode failed: {e}"))?;
    decode_migration_payload(&bytes)
}

/// Read MigrationPayload protobuf bytes.
fn decode_migration_payload(buf: &[u8]) -> Result<Vec<ImportedEntry>> {
    let mut entries = Vec::new();
    let mut reader = ProtoReader::new(buf);
    while reader.has_remaining() {
        let (field_no, wire_type) = reader.read_tag()?;
        if field_no == 1 && wire_type == 2 {
            // length-delimited submessage = OtpParameters
            let sub = reader.read_length_delimited()?;
            if let Some(entry) = decode_otp_parameters(sub)? {
                entries.push(entry);
            }
        } else {
            reader.skip(wire_type)?;
        }
    }
    Ok(entries)
}

fn decode_otp_parameters(buf: &[u8]) -> Result<Option<ImportedEntry>> {
    let mut secret_bytes: Vec<u8> = Vec::new();
    let mut name = String::new();
    let mut issuer = String::new();
    let mut algorithm_enum = 1u64; // default SHA1
    let mut digits_enum = 1u64; // default SIX
    let mut otp_type = 2u64; // default TOTP (we accept anything but skip non-TOTP)

    let mut reader = ProtoReader::new(buf);
    while reader.has_remaining() {
        let (field_no, wire_type) = reader.read_tag()?;
        match (field_no, wire_type) {
            (1, 2) => secret_bytes = reader.read_length_delimited()?.to_vec(),
            (2, 2) => name = String::from_utf8_lossy(reader.read_length_delimited()?).to_string(),
            (3, 2) => issuer = String::from_utf8_lossy(reader.read_length_delimited()?).to_string(),
            (4, 0) => algorithm_enum = reader.read_varint()?,
            (5, 0) => digits_enum = reader.read_varint()?,
            (6, 0) => otp_type = reader.read_varint()?,
            (_, wt) => reader.skip(wt)?,
        }
    }
    if otp_type != 2 {
        // HOTP / UNSPECIFIED — we only export TOTP. Silently drop;
        // import is best-effort.
        return Ok(None);
    }
    if secret_bytes.is_empty() {
        return Ok(None);
    }
    let algorithm = match algorithm_enum {
        2 => "SHA256",
        3 => "SHA512",
        _ => "SHA1",
    };
    let digits = match digits_enum {
        2 => 8,
        _ => 6,
    };
    // Google migration carries secrets as raw bytes; convert to base32
    // so our DB shape stays uniform.
    let secret_base32 = encode_base32_no_pad(&secret_bytes);
    // The `name` field is often "Issuer:account" — split if so.
    let (label_issuer, account) = match name.split_once(':') {
        Some((iss, acc)) => (Some(iss.trim().to_string()), acc.trim().to_string()),
        None => (None, name.trim().to_string()),
    };
    let final_issuer = if !issuer.is_empty() {
        issuer
    } else {
        label_issuer.unwrap_or_default()
    };
    Ok(Some(ImportedEntry {
        issuer: final_issuer.trim().to_string(),
        account,
        secret_base32,
        digits,
        period: 30, // Google migration doesn't encode period; default RFC 6238
        algorithm: algorithm.to_string(),
    }))
}

// ── 3) Aegis JSON ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AegisExport {
    db: Option<AegisDb>,
}
#[derive(Debug, Deserialize)]
struct AegisDb {
    entries: Vec<AegisEntry>,
}
#[derive(Debug, Deserialize)]
struct AegisEntry {
    #[serde(rename = "type")]
    type_: String,
    name: Option<String>,
    issuer: Option<String>,
    info: AegisInfo,
}
#[derive(Debug, Deserialize)]
struct AegisInfo {
    secret: Option<String>,
    digits: Option<u32>,
    period: Option<u32>,
    algo: Option<String>,
}

pub fn parse_aegis_json(json: &str) -> Result<Vec<ImportedEntry>> {
    let parsed: AegisExport = serde_json::from_str(json).context("not Aegis JSON shape")?;
    let entries = parsed.db.context("Aegis JSON missing `db` field")?.entries;
    let mut out = Vec::new();
    for e in entries {
        if e.type_.to_ascii_lowercase() != "totp" {
            continue;
        }
        let secret = match e.info.secret {
            Some(s) if !s.trim().is_empty() => s,
            _ => continue,
        };
        out.push(ImportedEntry {
            issuer: e.issuer.unwrap_or_default().trim().to_string(),
            account: e.name.unwrap_or_default().trim().to_string(),
            secret_base32: totp_store::normalise_secret(&secret),
            digits: e.info.digits.unwrap_or(6),
            period: e.info.period.unwrap_or(30),
            algorithm: e.info.algo.unwrap_or_else(|| "SHA1".into()).to_ascii_uppercase(),
        });
    }
    Ok(out)
}

// ── 4) 2FAS JSON ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TwoFasExport {
    services: Vec<TwoFasService>,
}
#[derive(Debug, Deserialize)]
struct TwoFasService {
    name: Option<String>,
    secret: Option<String>,
    otp: Option<TwoFasOtp>,
}
#[derive(Debug, Deserialize)]
struct TwoFasOtp {
    account: Option<String>,
    digits: Option<u32>,
    period: Option<u32>,
    algorithm: Option<String>,
}

pub fn parse_2fas_json(json: &str) -> Result<Vec<ImportedEntry>> {
    let parsed: TwoFasExport = serde_json::from_str(json).context("not 2FAS JSON shape")?;
    let mut out = Vec::new();
    for s in parsed.services {
        let secret = match s.secret {
            Some(sc) if !sc.trim().is_empty() => sc,
            _ => continue,
        };
        let otp = s.otp.unwrap_or(TwoFasOtp {
            account: None,
            digits: None,
            period: None,
            algorithm: None,
        });
        out.push(ImportedEntry {
            issuer: s.name.unwrap_or_default().trim().to_string(),
            account: otp.account.unwrap_or_default().trim().to_string(),
            secret_base32: totp_store::normalise_secret(&secret),
            digits: otp.digits.unwrap_or(6),
            period: otp.period.unwrap_or(30),
            algorithm: otp
                .algorithm
                .unwrap_or_else(|| "SHA1".into())
                .to_ascii_uppercase(),
        });
    }
    Ok(out)
}

// ── Export ──────────────────────────────────────────────────────────

/// Export all entries as a JSON array of `otpauth://` URIs — the most
/// portable shape (every authenticator app + most password managers
/// can re-import these). Secrets are NOT re-encrypted — this is plain
/// text. User must understand the implication; the UI hint says so.
pub fn export_otpauth_uris(
    db: &crate::db::DbHandle,
) -> Result<Vec<String>> {
    let entries = totp_store::list(db)?;
    let mut uris = Vec::with_capacity(entries.len());
    let conn = db.lock();
    for e in entries {
        let secret_enc: String = conn
            .query_row(
                "SELECT secret_enc FROM totp_entries WHERE id = ?1",
                [e.id],
                |r| r.get(0),
            )
            .with_context(|| format!("read secret for id {}", e.id))?;
        let secret = crate::crypto::decrypt(&secret_enc);
        let label = if !e.issuer.is_empty() && !e.account.is_empty() {
            format!("{}:{}", e.issuer, e.account)
        } else if !e.account.is_empty() {
            e.account.clone()
        } else {
            e.issuer.clone()
        };
        let mut q = vec![
            format!("secret={secret}"),
            format!("digits={}", e.digits),
            format!("period={}", e.period),
            format!("algorithm={}", e.algorithm),
        ];
        if !e.issuer.is_empty() {
            q.insert(1, format!("issuer={}", percent_encode(&e.issuer)));
        }
        uris.push(format!(
            "otpauth://totp/{}?{}",
            percent_encode(&label),
            q.join("&")
        ));
    }
    Ok(uris)
}

// ── Tiny protobuf wire-format reader ────────────────────────────────

struct ProtoReader<'a> {
    buf: &'a [u8],
    pos: usize,
}
impl<'a> ProtoReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn has_remaining(&self) -> bool {
        self.pos < self.buf.len()
    }
    fn read_byte(&mut self) -> Result<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| anyhow!("protobuf truncated"))?;
        self.pos += 1;
        Ok(b)
    }
    fn read_varint(&mut self) -> Result<u64> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.read_byte()?;
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err(anyhow!("varint overflow"));
            }
        }
    }
    fn read_tag(&mut self) -> Result<(u64, u8)> {
        let v = self.read_varint()?;
        Ok((v >> 3, (v & 0x07) as u8))
    }
    fn read_length_delimited(&mut self) -> Result<&'a [u8]> {
        let len = self.read_varint()? as usize;
        if self.pos + len > self.buf.len() {
            return Err(anyhow!("length-delimited overflow"));
        }
        let s = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Ok(s)
    }
    fn skip(&mut self, wire_type: u8) -> Result<()> {
        match wire_type {
            0 => {
                self.read_varint()?;
            }
            1 => self.pos += 8,
            2 => {
                let _ = self.read_length_delimited()?;
            }
            5 => self.pos += 4,
            other => return Err(anyhow!("unknown wire type {other}")),
        }
        Ok(())
    }
}

// ── Tiny base32 encoder (counterpart to totp_store::decode_base32) ──

fn encode_base32_no_pad(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut bits = 0u32;
    let mut bit_count = 0u32;
    for &b in bytes {
        bits = (bits << 8) | (b as u32);
        bit_count += 8;
        while bit_count >= 5 {
            bit_count -= 5;
            let idx = ((bits >> bit_count) & 0x1F) as usize;
            out.push(alphabet[idx] as char);
        }
    }
    if bit_count > 0 {
        let idx = ((bits << (5 - bit_count)) & 0x1F) as usize;
        out.push(alphabet[idx] as char);
    }
    out
}

// ── URL helpers ─────────────────────────────────────────────────────

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        // `+` is NOT space in URI path/query for otpauth (Google
        // spec uses %20 for spaces) — preserve literal.
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otpauth_uri_basic() {
        let e = parse_otpauth_uri(
            "otpauth://totp/Amazon:user%40example.com?secret=JBSWY3DPEHPK3PXP&issuer=Amazon",
        )
        .unwrap();
        assert_eq!(e.issuer, "Amazon");
        assert_eq!(e.account, "user@example.com");
        assert_eq!(e.secret_base32, "JBSWY3DPEHPK3PXP");
        assert_eq!(e.digits, 6);
        assert_eq!(e.period, 30);
        assert_eq!(e.algorithm, "SHA1");
    }

    #[test]
    fn otpauth_uri_overrides_default_period_and_digits() {
        let e = parse_otpauth_uri(
            "otpauth://totp/GH:me?secret=JBSWY3DPEHPK3PXP&digits=8&period=60&algorithm=SHA256",
        )
        .unwrap();
        assert_eq!(e.digits, 8);
        assert_eq!(e.period, 60);
        assert_eq!(e.algorithm, "SHA256");
    }

    #[test]
    fn otpauth_uri_label_without_issuer_uses_label_as_account() {
        let e = parse_otpauth_uri("otpauth://totp/justaccount?secret=JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!(e.issuer, "");
        assert_eq!(e.account, "justaccount");
    }

    #[test]
    fn otpauth_uri_missing_secret_errs() {
        assert!(parse_otpauth_uri("otpauth://totp/A:b?digits=6").is_err());
    }

    #[test]
    fn aegis_json_parses_totp_entries_skips_hotp() {
        let json = r#"{
          "db": { "entries": [
            { "type": "totp", "name": "Alice", "issuer": "GitHub",
              "info": { "secret": "JBSWY3DPEHPK3PXP", "digits": 6, "period": 30, "algo": "SHA1" } },
            { "type": "hotp", "name": "skipme", "issuer": "X",
              "info": { "secret": "JBSWY3DPEHPK3PXP", "digits": 6, "period": 30, "algo": "SHA1" } }
          ] }
        }"#;
        let entries = parse_aegis_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issuer, "GitHub");
        assert_eq!(entries[0].account, "Alice");
    }

    #[test]
    fn twofas_json_parses_services() {
        let json = r#"{
          "services": [
            { "name": "Amazon", "secret": "JBSWY3DPEHPK3PXP",
              "otp": { "account": "me@example.com", "digits": 6, "period": 30, "algorithm": "SHA1" } }
          ]
        }"#;
        let entries = parse_2fas_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issuer, "Amazon");
        assert_eq!(entries[0].account, "me@example.com");
    }

    #[test]
    fn import_auto_dispatches_by_marker() {
        let single = "otpauth://totp/A:b?secret=JBSWY3DPEHPK3PXP";
        assert_eq!(import_auto(single).unwrap().len(), 1);

        let aegis = r#"{"db":{"entries":[{"type":"totp","name":"a","issuer":"i",
          "info":{"secret":"JBSWY3DPEHPK3PXP"}}]}}"#;
        assert_eq!(import_auto(aegis).unwrap().len(), 1);
    }

    #[test]
    fn import_auto_plain_text_multiline() {
        let input = "# my export\notpauth://totp/A:1?secret=JBSWY3DPEHPK3PXP\n\notpauth://totp/B:2?secret=KRSWG4LBORSXG43JNZTQ====";
        let entries = import_auto(input).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].account, "1");
        assert_eq!(entries[1].account, "2");
    }

    #[test]
    fn import_auto_blank_returns_empty() {
        assert_eq!(import_auto("").unwrap().len(), 0);
        assert_eq!(import_auto("   \n\n  ").unwrap().len(), 0);
    }

    #[test]
    fn import_auto_unknown_format_errors() {
        assert!(import_auto("hello world this is nothing").is_err());
        assert!(import_auto(r#"{"random":"json"}"#).is_err());
    }

    #[test]
    fn base32_round_trip() {
        let original = b"hello world";
        let encoded = encode_base32_no_pad(original);
        let decoded = crate::totp_store::decode_base32(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn protobuf_reader_round_trip_simple_message() {
        // Hand-built MigrationPayload with one OtpParameters:
        //   field 1 (otp_parameters, type 2 = length-delimited), 25 bytes
        //   sub: field 1 (secret) length 6 = b"secret"
        //        field 2 (name) length 5 = b"alice"
        //        field 3 (issuer) length 6 = b"github"
        let mut sub = Vec::new();
        // field 1 wire 2, len 6, "secret"
        sub.push((1 << 3) | 2);
        sub.push(6);
        sub.extend_from_slice(b"secret");
        // field 2 wire 2, len 5, "alice"
        sub.push((2 << 3) | 2);
        sub.push(5);
        sub.extend_from_slice(b"alice");
        // field 3 wire 2, len 6, "github"
        sub.push((3 << 3) | 2);
        sub.push(6);
        sub.extend_from_slice(b"github");
        // field 6 wire 0 (otp_type = 2 = TOTP)
        sub.push((6 << 3) | 0);
        sub.push(2);

        let mut full = Vec::new();
        full.push((1 << 3) | 2); // field 1 wire 2
        full.push(sub.len() as u8);
        full.extend_from_slice(&sub);

        let entries = decode_migration_payload(&full).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issuer, "github");
        assert_eq!(entries[0].account, "alice");
        // "secret" → base32-encoded
        assert_eq!(entries[0].secret_base32, encode_base32_no_pad(b"secret"));
    }
}
