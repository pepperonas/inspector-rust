//! `mailcheck <email>` — layered, HONEST e-mail deliverability checks.
//!
//! E-mail verification has hard limits: most mail servers deliberately prevent
//! recipient enumeration (they accept every RCPT, or reject probes), and
//! outbound port 25 is routinely blocked. So this never claims a mailbox
//! "exists" — it runs independent stages (syntax → domain → DNS/MX → mail
//! server → non-invasive SMTP) and reports each honestly, with an overall
//! status derived deterministically from the signals actually obtained.
//!
//! All network work is here in Rust with tight, bounded timeouts (§ "keine
//! UI-Blockierung"); the command runs it on `spawn_blocking`. It NEVER sends a
//! mail, authenticates, or issues aggressive probes. No secrets are logged
//! (there are none — DNS/SMTP need no credentials).
//!
//! Pure logic (syntax/domain/disposable/classification/DNS-parse/SMTP-reply) is
//! split from the impure sockets and unit-tested; the network shell is not.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const DNS_RESOLVERS: [&str; 2] = ["1.1.1.1:53", "8.8.8.8:53"];
const DNS_TIMEOUT: Duration = Duration::from_secs(3);
const SMTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const SMTP_IO_TIMEOUT: Duration = Duration::from_secs(6);
/// Try at most this many MX hosts before giving up (avoids long fan-out).
const MAX_SMTP_HOSTS: usize = 2;
/// A neutral HELO/EHLO identity — we control no domain, and a probe needs one.
const HELO_NAME: &str = "inspector-rust.local";

// ── data model (§ "saubere Datenmodelle") ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxStatus {
    LikelyValid,
    Unknown,
    Unlikely,
    Invalid,
    /// SMTP wasn't attempted (e.g. no MX, or a prior stage failed).
    NotProbed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatchAll {
    Yes,
    No,
    Unknown,
}

/// The composite headline (never says "email exists").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overall {
    /// A mailbox reply strongly indicates it accepts mail (RCPT accepted, not catch-all).
    Deliverable,
    /// The domain can receive mail (MX/A present) but the mailbox is unverifiable.
    DeliverableLikely,
    /// Deliverable-ish but flagged (disposable domain, or catch-all server).
    Risky,
    /// The domain cannot receive mail, or the mailbox was hard-rejected.
    Undeliverable,
    /// Syntactically invalid — nothing else attempted.
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MxRecord {
    pub preference: u16,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsResult {
    /// The domain resolves (has MX, or A/AAAA as an implicit mail exchanger).
    pub resolves: bool,
    pub mx: Vec<MxRecord>,
    /// True when there is no explicit MX but the A record acts as implicit MX.
    pub implicit_mx: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpResult {
    pub attempted: bool,
    pub connected: bool,
    pub tried_host: Option<String>,
    pub banner: Option<String>,
    /// The SMTP reply code to `RCPT TO:<target>`.
    pub rcpt_code: Option<u16>,
    pub mailbox: MailboxStatus,
    pub catch_all: CatchAll,
    /// A plain-language explanation (shown under the SMTP row).
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailCheckResult {
    pub email: String,
    pub syntax_valid: bool,
    pub domain: Option<String>,
    pub disposable: bool,
    pub dns: DnsResult,
    pub smtp: SmtpResult,
    pub overall: Overall,
}

// ── pure logic ──

/// Robust-but-pragmatic syntax check. Not full RFC 5322 (that accepts things no
/// real address uses), but rejects the mistakes that matter: missing/multiple
/// `@`, empty local/domain, spaces, a domain without a dot or with bad labels.
pub fn validate_syntax(email: &str) -> bool {
    let email = email.trim();
    if email.len() > 254 || email.contains(char::is_whitespace) {
        return false;
    }
    let mut parts = email.rsplitn(2, '@');
    let domain = parts.next().unwrap_or("");
    let local = match parts.next() {
        Some(l) => l,
        None => return false, // no '@'
    };
    if local.is_empty() || local.len() > 64 || email.matches('@').count() != 1 {
        return false;
    }
    // local part: allow the common atext + dot, no leading/trailing/double dot
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    let local_ok = local.chars().all(|c| {
        c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c)
    });
    local_ok && valid_domain(domain)
}

/// A syntactically valid DNS domain: ≥2 dot-separated labels, each 1–63 chars of
/// letters/digits/hyphen (not leading/trailing hyphen), total ≤253.
pub fn valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 || !domain.contains('.') {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// A curated set of well-known disposable / throwaway mail domains. Not
/// exhaustive (the space is endless) — a useful heads-up, not a guarantee.
const DISPOSABLE: &[&str] = &[
    "mailinator.com", "guerrillamail.com", "guerrillamail.info", "sharklasers.com",
    "10minutemail.com", "10minutemail.net", "tempmail.com", "temp-mail.org",
    "throwawaymail.com", "yopmail.com", "yopmail.fr", "getnada.com", "nada.email",
    "trashmail.com", "trashmail.de", "mailnesia.com", "maildrop.cc", "dispostable.com",
    "fakeinbox.com", "spam4.me", "mytemp.email", "moakt.com", "mohmal.com",
    "temp-mail.io", "tempmailo.com", "emailondeck.com", "throwawaymail.net",
    "gettempmail.com", "tempinbox.com", "burnermail.io", "33mail.com", "mailcatch.com",
    "inboxbear.com", "tempr.email", "discard.email", "spamgourmet.com", "mailexpire.com",
    "anonbox.net", "harakirimail.com", "mvrht.com", "grr.la", "guerrillamailblock.com",
];

/// Is `domain` a known disposable-mail domain (exact, or a subdomain of one)?
pub fn is_disposable(domain: &str) -> bool {
    let d = domain.trim().to_ascii_lowercase();
    DISPOSABLE
        .iter()
        .any(|&base| d == base || d.ends_with(&format!(".{base}")))
}

/// Classify an SMTP `RCPT TO` reply code into a mailbox verdict.
pub fn classify_rcpt(code: u16) -> MailboxStatus {
    match code {
        250 | 251 => MailboxStatus::LikelyValid, // accepted (⚠️ may be catch-all)
        550 | 551 | 553 => MailboxStatus::Invalid, // no such user
        552 | 554 => MailboxStatus::Unlikely,      // rejected (policy/other)
        450..=452 => MailboxStatus::Unknown, // greylist / temp — inconclusive
        _ => MailboxStatus::Unknown,
    }
}

/// Derive the composite headline from the stage signals (deterministic, tested).
pub fn derive_overall(
    syntax_valid: bool,
    resolves: bool,
    mailbox: MailboxStatus,
    catch_all: CatchAll,
    disposable: bool,
) -> Overall {
    if !syntax_valid {
        return Overall::Invalid;
    }
    if !resolves {
        return Overall::Undeliverable; // domain cannot receive mail
    }
    match mailbox {
        MailboxStatus::Invalid => Overall::Undeliverable,
        MailboxStatus::LikelyValid if catch_all != CatchAll::Yes => {
            if disposable {
                Overall::Risky
            } else {
                Overall::Deliverable
            }
        }
        // accepted-but-catch-all, unlikely, unknown, or not probed → the domain
        // can receive mail but the mailbox itself is unverified.
        _ => {
            if disposable || catch_all == CatchAll::Yes {
                Overall::Risky
            } else {
                Overall::DeliverableLikely
            }
        }
    }
}

// ── DNS over UDP (minimal, defensive; untrusted network input) ──

mod dns {
    use super::{MxRecord, DNS_RESOLVERS, DNS_TIMEOUT};
    use std::net::UdpSocket;

    /// Build a DNS query for `domain` of `qtype` (15 = MX, 1 = A).
    pub fn build_query(id: u16, domain: &str, qtype: u16) -> Vec<u8> {
        let mut q = Vec::with_capacity(32);
        q.extend_from_slice(&id.to_be_bytes());
        q.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: recursion desired
        q.extend_from_slice(&1u16.to_be_bytes()); // qdcount
        q.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // an/ns/ar counts
        for label in domain.trim_end_matches('.').split('.') {
            let bytes = label.as_bytes();
            q.push(bytes.len().min(63) as u8);
            q.extend_from_slice(&bytes[..bytes.len().min(63)]);
        }
        q.push(0); // root
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes()); // class IN
        q
    }

    /// Read a (possibly compressed) DNS name starting at `pos`. Returns the name
    /// and the offset just past the name in the ORIGINAL stream. Loop-guarded.
    pub fn read_name(buf: &[u8], start: usize) -> Option<(String, usize)> {
        let mut labels: Vec<String> = Vec::new();
        let mut pos = start;
        let mut next: Option<usize> = None;
        let mut jumps = 0;
        loop {
            let len = *buf.get(pos)?;
            if len & 0xc0 == 0xc0 {
                let b2 = *buf.get(pos + 1)? as usize;
                let ptr = (((len & 0x3f) as usize) << 8) | b2;
                if next.is_none() {
                    next = Some(pos + 2);
                }
                jumps += 1;
                if jumps > 32 || ptr >= buf.len() {
                    return None; // malformed / compression loop
                }
                pos = ptr;
                continue;
            }
            if len == 0 {
                pos += 1;
                break;
            }
            let s = pos + 1;
            let e = s + len as usize;
            if e > buf.len() {
                return None;
            }
            labels.push(String::from_utf8_lossy(&buf[s..e]).into_owned());
            pos = e;
        }
        Some((labels.join("."), next.unwrap_or(pos)))
    }

    /// Parse the MX answers out of a DNS response. Never panics on malformed
    /// input — a bad record is skipped, the rest still parse.
    pub fn parse_mx(buf: &[u8], expect_id: u16) -> Option<Vec<MxRecord>> {
        if buf.len() < 12 {
            return None;
        }
        let id = u16::from_be_bytes([buf[0], buf[1]]);
        if id != expect_id {
            return None; // not our reply
        }
        let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
        let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
        let mut pos = 12;
        for _ in 0..qd {
            let (_, p) = read_name(buf, pos)?;
            pos = p + 4; // qtype + qclass
        }
        let mut out = Vec::new();
        for _ in 0..an {
            let (_, p) = read_name(buf, pos)?;
            if p + 10 > buf.len() {
                break;
            }
            let rtype = u16::from_be_bytes([buf[p], buf[p + 1]]);
            let rdlen = u16::from_be_bytes([buf[p + 8], buf[p + 9]]) as usize;
            let rdata = p + 10;
            if rdata + rdlen > buf.len() {
                break;
            }
            if rtype == 15 && rdlen >= 3 {
                let preference = u16::from_be_bytes([buf[rdata], buf[rdata + 1]]);
                if let Some((host, _)) = read_name(buf, rdata + 2) {
                    if !host.is_empty() {
                        out.push(MxRecord { preference, host });
                    }
                }
            }
            pos = rdata + rdlen;
        }
        out.sort_by_key(|r| r.preference);
        Some(out)
    }

    /// Query the configured resolvers for the domain's MX records. Returns
    /// `Some(vec)` (possibly empty) on a parseable reply, `None` on total
    /// network failure.
    pub fn query_mx(domain: &str) -> Option<Vec<MxRecord>> {
        let id: u16 = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            & 0xffff) as u16;
        let query = build_query(id, domain, 15);
        for resolver in DNS_RESOLVERS {
            if let Some(records) = try_resolver(resolver, &query, id) {
                return Some(records);
            }
        }
        None
    }

    fn try_resolver(resolver: &str, query: &[u8], id: u16) -> Option<Vec<MxRecord>> {
        let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
        sock.set_read_timeout(Some(DNS_TIMEOUT)).ok()?;
        sock.set_write_timeout(Some(DNS_TIMEOUT)).ok()?;
        sock.connect(resolver).ok()?;
        sock.send(query).ok()?;
        let mut buf = [0u8; 1500];
        let n = sock.recv(&mut buf).ok()?;
        parse_mx(&buf[..n], id)
    }
}

// ── DNS + SMTP orchestration (impure) ──

fn resolve(domain: &str) -> DnsResult {
    let mx = dns::query_mx(domain).unwrap_or_default();
    // A/AAAA resolution via the OS resolver — for "does the domain resolve" and
    // the implicit-MX fallback (RFC 5321 §5.1: no MX ⇒ the A record is the MX).
    let has_a = (domain, 25u16).to_socket_addrs().map(|mut i| i.next().is_some()).unwrap_or(false);
    if !mx.is_empty() {
        DnsResult { resolves: true, mx, implicit_mx: false, note: None }
    } else if has_a {
        DnsResult {
            resolves: true,
            mx: vec![MxRecord { preference: 0, host: domain.to_string() }],
            implicit_mx: true,
            note: Some("No MX record — using the A record as an implicit mail exchanger.".into()),
        }
    } else {
        DnsResult {
            resolves: false,
            mx: Vec::new(),
            implicit_mx: false,
            note: Some("The domain has no MX or address record — it cannot receive mail.".into()),
        }
    }
}

/// Read one (possibly multiline) SMTP reply; returns its 3-digit code.
fn read_reply(reader: &mut impl BufRead) -> Option<u16> {
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 3 {
            continue;
        }
        let code: u16 = line.get(0..3)?.parse().ok()?;
        // "250-" = more lines follow; "250 " (or bare) = final line.
        let more = bytes.get(3) == Some(&b'-');
        if !more {
            return Some(code);
        }
    }
}

fn send(stream: &mut TcpStream, line: &str) -> std::io::Result<()> {
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\r\n")?;
    stream.flush()
}

/// A non-invasive SMTP probe: connect, EHLO, MAIL FROM:<>, RCPT TO:<target>,
/// plus ONE catch-all RCPT in the SAME session, then QUIT. Never DATA, never
/// AUTH. Returns a differentiated result — most servers are inconclusive.
fn probe_smtp(email: &str, domain: &str, mx: &[MxRecord]) -> SmtpResult {
    let mut result = SmtpResult {
        attempted: true,
        connected: false,
        tried_host: None,
        banner: None,
        rcpt_code: None,
        mailbox: MailboxStatus::Unknown,
        catch_all: CatchAll::Unknown,
        note: String::new(),
    };
    if mx.is_empty() {
        result.attempted = false;
        result.mailbox = MailboxStatus::NotProbed;
        result.note = "No mail server to probe.".into();
        return result;
    }

    let random_local = random_local_part();
    for record in mx.iter().take(MAX_SMTP_HOSTS) {
        result.tried_host = Some(record.host.clone());
        let addrs = match (record.host.as_str(), 25u16).to_socket_addrs() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let Some(addr) = addrs.into_iter().next() else { continue };
        let stream = match TcpStream::connect_timeout(&addr, SMTP_CONNECT_TIMEOUT) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = stream.set_read_timeout(Some(SMTP_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(SMTP_IO_TIMEOUT));
        let mut writer = stream.try_clone().expect("clone smtp stream");
        let mut reader = BufReader::new(stream);

        // Banner
        match read_reply(&mut reader) {
            Some(code) if (200..400).contains(&code) => {
                result.connected = true;
            }
            _ => {
                result.note = "Mail server did not present a usable greeting.".into();
                continue;
            }
        }
        // EHLO (best-effort; proceed even if a server is terse)
        let _ = send(&mut writer, &format!("EHLO {HELO_NAME}"));
        let _ = read_reply(&mut reader);
        // MAIL FROM: null sender (the bounce address) — standard, non-invasive.
        let _ = send(&mut writer, "MAIL FROM:<>");
        let _ = read_reply(&mut reader);
        // RCPT TO the real target.
        if send(&mut writer, &format!("RCPT TO:<{email}>")).is_ok() {
            result.rcpt_code = read_reply(&mut reader);
        }
        // ONE catch-all probe in the same session (not a new connection).
        let mut catch_code = None;
        if send(&mut writer, &format!("RCPT TO:<{random_local}@{domain}>")).is_ok() {
            catch_code = read_reply(&mut reader);
        }
        let _ = send(&mut writer, "QUIT");

        result.mailbox = result.rcpt_code.map(classify_rcpt).unwrap_or(MailboxStatus::Unknown);
        result.catch_all = match (result.rcpt_code, catch_code) {
            (Some(r), Some(c)) if accepted(r) && accepted(c) => CatchAll::Yes,
            (Some(r), Some(c)) if accepted(r) && !accepted(c) => CatchAll::No,
            _ => CatchAll::Unknown,
        };
        result.note = smtp_note(&result);
        return result;
    }

    // No host connected — almost always outbound port 25 blocked.
    result.note =
        "Could not reach any mail server on port 25 (often blocked by ISPs/cloud networks). \
         Mailbox existence cannot be checked."
            .into();
    result.mailbox = MailboxStatus::Unknown;
    result
}

fn accepted(code: u16) -> bool {
    matches!(code, 250 | 251)
}

fn smtp_note(r: &SmtpResult) -> String {
    match (r.mailbox, r.catch_all) {
        (MailboxStatus::LikelyValid, CatchAll::Yes) => {
            "The server accepts every recipient (catch-all), so acceptance does not confirm this specific mailbox.".into()
        }
        (MailboxStatus::LikelyValid, _) => {
            "The mail server accepted this recipient. Many servers accept without confirming existence.".into()
        }
        (MailboxStatus::Invalid, _) => "The mail server rejected this recipient as unknown.".into(),
        (MailboxStatus::Unlikely, _) => "The mail server rejected this recipient.".into(),
        _ => "The mail server does not reveal whether this mailbox exists.".into(),
    }
}

fn random_local_part() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect();
    (0..16).map(|_| chars[rng.gen_range(0..chars.len())]).collect()
}

/// The full layered check (§1–§5). Bounded by the per-stage timeouts above;
/// short-circuits on invalid syntax / non-resolving domain so it never hangs.
pub fn check(email: &str) -> MailCheckResult {
    let email = email.trim().to_string();
    let syntax_valid = validate_syntax(&email);
    let domain = if syntax_valid {
        Some(email.rsplit('@').next().unwrap_or("").to_ascii_lowercase())
    } else {
        None
    };
    let disposable = domain.as_deref().map(is_disposable).unwrap_or(false);

    let (dns, smtp) = match &domain {
        Some(d) if syntax_valid => {
            let dns = resolve(d);
            let smtp = if dns.resolves {
                probe_smtp(&email, d, &dns.mx)
            } else {
                SmtpResult {
                    attempted: false,
                    connected: false,
                    tried_host: None,
                    banner: None,
                    rcpt_code: None,
                    mailbox: MailboxStatus::NotProbed,
                    catch_all: CatchAll::Unknown,
                    note: "Not probed — the domain cannot receive mail.".into(),
                }
            };
            (dns, smtp)
        }
        _ => (
            DnsResult { resolves: false, mx: Vec::new(), implicit_mx: false, note: None },
            SmtpResult {
                attempted: false,
                connected: false,
                tried_host: None,
                banner: None,
                rcpt_code: None,
                mailbox: MailboxStatus::NotProbed,
                catch_all: CatchAll::Unknown,
                note: "Not probed — invalid address.".into(),
            },
        ),
    };

    let overall = derive_overall(syntax_valid, dns.resolves, smtp.mailbox, smtp.catch_all, disposable);
    MailCheckResult { email, syntax_valid, domain, disposable, dns, smtp, overall }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_accepts_normal_and_rejects_broken() {
        assert!(validate_syntax("hello@example.com"));
        assert!(validate_syntax("a.b+tag@sub.example.co.uk"));
        assert!(!validate_syntax("plainaddress"));
        assert!(!validate_syntax("no@domain")); // no dot
        assert!(!validate_syntax("two@@example.com"));
        assert!(!validate_syntax("@example.com"));
        assert!(!validate_syntax("a@b@example.com"));
        assert!(!validate_syntax("spaces in@example.com"));
        assert!(!validate_syntax(".dot@example.com"));
        assert!(!validate_syntax("dot.@example.com"));
        assert!(!validate_syntax("do..t@example.com"));
    }

    #[test]
    fn domain_validation() {
        assert!(valid_domain("example.com"));
        assert!(valid_domain("a.b.c.example.io"));
        assert!(!valid_domain("nodot"));
        assert!(!valid_domain("-bad.com"));
        assert!(!valid_domain("bad-.com"));
        assert!(!valid_domain("has_underscore.com"));
    }

    #[test]
    fn disposable_detection_exact_and_subdomain() {
        assert!(is_disposable("mailinator.com"));
        assert!(is_disposable("MAILINATOR.COM"));
        assert!(is_disposable("foo.guerrillamail.com")); // subdomain
        assert!(!is_disposable("example.com"));
        assert!(!is_disposable("notmailinator.com")); // must be exact base or .base
    }

    #[test]
    fn classify_rcpt_codes() {
        assert_eq!(classify_rcpt(250), MailboxStatus::LikelyValid);
        assert_eq!(classify_rcpt(251), MailboxStatus::LikelyValid);
        assert_eq!(classify_rcpt(550), MailboxStatus::Invalid);
        assert_eq!(classify_rcpt(552), MailboxStatus::Unlikely);
        assert_eq!(classify_rcpt(451), MailboxStatus::Unknown);
        assert_eq!(classify_rcpt(220), MailboxStatus::Unknown);
    }

    #[test]
    fn overall_status_is_derived_honestly() {
        // invalid syntax short-circuits
        assert_eq!(
            derive_overall(false, false, MailboxStatus::NotProbed, CatchAll::Unknown, false),
            Overall::Invalid
        );
        // no DNS = undeliverable
        assert_eq!(
            derive_overall(true, false, MailboxStatus::NotProbed, CatchAll::Unknown, false),
            Overall::Undeliverable
        );
        // MX present, mailbox unverifiable (the common, honest case)
        assert_eq!(
            derive_overall(true, true, MailboxStatus::Unknown, CatchAll::Unknown, false),
            Overall::DeliverableLikely
        );
        // RCPT accepted, not catch-all → strong
        assert_eq!(
            derive_overall(true, true, MailboxStatus::LikelyValid, CatchAll::No, false),
            Overall::Deliverable
        );
        // accepted but catch-all → risky, not "deliverable"
        assert_eq!(
            derive_overall(true, true, MailboxStatus::LikelyValid, CatchAll::Yes, false),
            Overall::Risky
        );
        // hard reject → undeliverable
        assert_eq!(
            derive_overall(true, true, MailboxStatus::Invalid, CatchAll::No, false),
            Overall::Undeliverable
        );
        // disposable domain flags an otherwise-strong result as risky
        assert_eq!(
            derive_overall(true, true, MailboxStatus::LikelyValid, CatchAll::No, true),
            Overall::Risky
        );
    }

    #[test]
    fn dns_query_roundtrips_and_parses_mx() {
        let id = 0x1234;
        let q = dns::build_query(id, "example.com", 15);
        // header id + RD flag + qdcount=1
        assert_eq!(&q[0..2], &id.to_be_bytes());
        assert_eq!(&q[2..4], &0x0100u16.to_be_bytes());
        assert_eq!(&q[4..6], &1u16.to_be_bytes());
        // qname "example.com" encoded, then qtype MX (15) + class IN
        assert!(q.ends_with(&[0, 15, 0, 1]));

        // Build a synthetic response: header (id, qr+ra flags, qd=1, an=2),
        // question, then two MX answers using name compression to the qname.
        let mut r = Vec::new();
        r.extend_from_slice(&id.to_be_bytes());
        r.extend_from_slice(&0x8180u16.to_be_bytes()); // response, recursion available
        r.extend_from_slice(&1u16.to_be_bytes()); // qd
        r.extend_from_slice(&2u16.to_be_bytes()); // an
        r.extend_from_slice(&[0, 0, 0, 0]);
        // question: example.com MX IN  (qname starts at offset 12)
        let qname_off = r.len();
        for l in ["example", "com"] {
            r.push(l.len() as u8);
            r.extend_from_slice(l.as_bytes());
        }
        r.push(0);
        r.extend_from_slice(&15u16.to_be_bytes());
        r.extend_from_slice(&1u16.to_be_bytes());
        // answer 1: name=ptr to qname, MX, ttl, rdata = pref(20) + host mx2.<ptr>
        let mut answer = |pref: u16, host_label: &str| {
            r.extend_from_slice(&[0xc0, qname_off as u8]); // name ptr
            r.extend_from_slice(&15u16.to_be_bytes()); // type MX
            r.extend_from_slice(&1u16.to_be_bytes()); // class
            r.extend_from_slice(&300u32.to_be_bytes()); // ttl
            // rdata: preference + host (label + pointer to qname)
            let mut rd = Vec::new();
            rd.extend_from_slice(&pref.to_be_bytes());
            rd.push(host_label.len() as u8);
            rd.extend_from_slice(host_label.as_bytes());
            rd.extend_from_slice(&[0xc0, qname_off as u8]);
            r.extend_from_slice(&(rd.len() as u16).to_be_bytes());
            r.extend_from_slice(&rd);
        };
        answer(20, "mx2");
        answer(10, "mx1");

        let mx = dns::parse_mx(&r, id).expect("parses");
        assert_eq!(mx.len(), 2);
        // sorted by preference ascending
        assert_eq!(mx[0], MxRecord { preference: 10, host: "mx1.example.com".into() });
        assert_eq!(mx[1], MxRecord { preference: 20, host: "mx2.example.com".into() });
    }

    #[test]
    fn dns_parse_rejects_wrong_id_and_survives_garbage() {
        assert!(dns::parse_mx(&[0u8; 4], 1).is_none()); // too short
        let mut r = vec![0x12, 0x34];
        r.extend_from_slice(&[0u8; 20]);
        assert!(dns::parse_mx(&r, 0x9999).is_none()); // id mismatch
        // random bytes must never panic
        for seed in 0u32..200 {
            let mut x = seed.wrapping_mul(2654435761);
            let mut buf = 0x1234u16.to_be_bytes().to_vec();
            buf.extend_from_slice(&[0x81, 0x80, 0, 1, 0, 5, 0, 0, 0, 0]);
            for _ in 0..(seed % 60) {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                buf.push((x & 0xff) as u8);
            }
            let _ = dns::parse_mx(&buf, 0x1234);
        }
    }

    #[test]
    fn read_name_handles_compression_pointer() {
        // "a" then pointer back to offset 0 ("a")… actually build: at 0: "hi",0
        let mut buf = Vec::new();
        buf.push(2);
        buf.extend_from_slice(b"hi");
        buf.push(0); // name "hi" at offset 0, ends at 3
        let base = buf.len();
        // at `base`: "x" + pointer to 0
        buf.push(1);
        buf.push(b'x');
        buf.extend_from_slice(&[0xc0, 0x00]);
        let (name, next) = dns::read_name(&buf, base).unwrap();
        assert_eq!(name, "x.hi");
        assert_eq!(next, base + 4); // past the 2-byte pointer
    }

    #[test]
    fn read_reply_reads_multiline() {
        use std::io::BufReader;
        let data = b"250-first\r\n250-second\r\n250 done\r\n";
        let mut r = BufReader::new(&data[..]);
        assert_eq!(read_reply(&mut r), Some(250));
    }

    #[test]
    fn check_invalid_syntax_short_circuits_without_network() {
        let res = check("not-an-email");
        assert!(!res.syntax_valid);
        assert_eq!(res.overall, Overall::Invalid);
        assert!(!res.smtp.attempted);
        assert!(res.dns.mx.is_empty());
    }
}
