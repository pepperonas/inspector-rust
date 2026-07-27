//! The tool registry — the single source of truth. Each `ToolSpec` is pure
//! data; adding a tool (see [`GOBUSTER`]) needs no parser/builder/UI change.
//!
//! Flags verified against the current official sources (feroxbuster README,
//! John MODES + `run/*2john`, nmap/sqlmap docs) — not guessed.

use super::{FieldSpec, JohnFormat, PresetSpec, Segment, ToolSpec};
use Segment::{Field, Flag, Joined, Lit};

/// Common wordlist paths shipped by Kali / SecLists, offered as autocomplete in
/// the wordlist field. The frontend checks which actually exist on this box
/// (`sec_path_exists`) and prefers those.
pub static COMMON_WORDLISTS: &[&str] = &[
    "/usr/share/wordlists/rockyou.txt",
    "/usr/share/seclists/Discovery/Web-Content/directory-list-2.3-medium.txt",
    "/usr/share/seclists/Discovery/Web-Content/common.txt",
    "/usr/share/seclists/Discovery/Web-Content/raft-medium-directories.txt",
    "/usr/share/seclists/Passwords/Leaked-Databases/rockyou.txt",
    "/usr/share/wordlists/dirb/common.txt",
    "/usr/share/wordlists/dirbuster/directory-list-2.3-medium.txt",
];

/// Verified John `--format=` names people actually reach for. `jumbo` marks the
/// ones only the Jumbo build supports (Core has just a handful of crypt/LM
/// formats); the frontend hides jumbo names when the Core line is selected.
pub static JOHN_FORMATS: &[JohnFormat] = &[
    // Core (available in the stock openwall build)
    JohnFormat { name: "descrypt", jumbo: false },
    JohnFormat { name: "md5crypt", jumbo: false },
    JohnFormat { name: "bcrypt", jumbo: false },
    JohnFormat { name: "lm", jumbo: false },
    JohnFormat { name: "bsdicrypt", jumbo: false },
    // Jumbo
    JohnFormat { name: "raw-md5", jumbo: true },
    JohnFormat { name: "raw-sha1", jumbo: true },
    JohnFormat { name: "raw-sha256", jumbo: true },
    JohnFormat { name: "raw-sha512", jumbo: true },
    JohnFormat { name: "sha256crypt", jumbo: true },
    JohnFormat { name: "sha512crypt", jumbo: true },
    JohnFormat { name: "nt", jumbo: true },
    JohnFormat { name: "netntlmv2", jumbo: true },
    JohnFormat { name: "krb5tgs", jumbo: true },
    JohnFormat { name: "krb5asrep", jumbo: true },
    JohnFormat { name: "wpapsk", jumbo: true },
    JohnFormat { name: "mscash2", jumbo: true },
    JohnFormat { name: "phpass", jumbo: true },
    JohnFormat { name: "mysql-sha1", jumbo: true },
    JohnFormat { name: "zip", jumbo: true },
    JohnFormat { name: "rar", jumbo: true },
    JohnFormat { name: "7z", jumbo: true },
    JohnFormat { name: "pdf", jumbo: true },
    JohnFormat { name: "ssh", jumbo: true },
    JohnFormat { name: "gpg", jumbo: true },
    JohnFormat { name: "keepass", jumbo: true },
];

// ── nmap ─────────────────────────────────────────────────────────────────────

static NMAP_FIELDS: &[FieldSpec] = &[
    FieldSpec { key: "target", label: "Target", placeholder: "host / CIDR / range", help: "Host, CIDR (10.0.0.0/24) or range (10.0.0.1-50)", required: true },
    FieldSpec { key: "ports", label: "Ports", placeholder: "e.g. 22,80,443 or 1-1000", help: "-p — port(s)/range to scan", required: false },
    FieldSpec { key: "timing", label: "Timing", placeholder: "0–5", help: "-T — 0 (paranoid) … 5 (insane, loud)", required: false },
    FieldSpec { key: "output", label: "Output base", placeholder: "scan", help: "-oA — write .nmap/.gnmap/.xml to <base>", required: false },
];

// Each preset = its literal flags, then the shared optional fields, then the
// positional target. (Optionals are dropped when unset by the builder.)
macro_rules! nmap_preset {
    ($name:literal, $aliases:expr, $purpose:literal, [$($lit:literal),*], $sharp:expr, $tags:expr) => {
        PresetSpec {
            name: $name, aliases: $aliases, purpose: $purpose,
            segments: &[$(Lit { text: $lit },)* Flag { flag: "-p", key: "ports" },
                        Joined { prefix: "-T", key: "timing" }, Flag { flag: "-oA", key: "output" },
                        Field { key: "target" }],
            fields: &["target", "ports", "timing", "output"], sharp: $sharp, tags: $tags,
            category: "scan",
        }
    };
}

static NMAP_PRESETS: &[PresetSpec] = &[
    nmap_preset!("quick", &["fast", "f"], "Fast scan — only the 100 most common ports", ["nmap", "-F"], false, &[]),
    nmap_preset!("top", &["default"], "Service/version detection on the top 1000 ports", ["nmap", "-sV"], false, &[]),
    nmap_preset!("service", &["vers", "sc"], "Version + default NSE scripts", ["nmap", "-sV", "-sC"], false, &[]),
    nmap_preset!("full-tcp", &["allports", "full"], "Scan all 65535 TCP ports (slow)", ["nmap", "-p-"], true, &["long-running"]),
    nmap_preset!("udp-top", &["udp"], "UDP scan of the top ports (slow)", ["nmap", "-sU"], false, &["long-running"]),
    nmap_preset!("os", &["osdetect"], "OS detection (needs root)", ["nmap", "-O"], false, &["needs-root"]),
    nmap_preset!("ping-sweep", &["ping", "discovery", "sweep"], "Host discovery only — no port scan", ["nmap", "-sn"], false, &[]),
    nmap_preset!("vuln", &["vulns"], "Run the 'vuln' NSE scripts (loud, slow)", ["nmap", "--script", "vuln"], false, &["loud", "long-running"]),
    nmap_preset!("stealth-syn", &["syn", "stealth"], "TCP SYN half-open scan (needs root)", ["nmap", "-sS"], false, &["needs-root"]),
];

static NMAP_FLAG_HELP: &[(&str, &str)] = &[
    ("-F", "Fast scan — only the 100 most common ports"),
    ("-sV", "Service/version detection"),
    ("-sC", "Run the default (safe) NSE scripts"),
    ("-p-", "Scan all 65535 TCP ports"),
    ("-sU", "UDP scan"),
    ("-O", "OS detection (needs root)"),
    ("-sn", "Host discovery only — no port scan ('ping sweep')"),
    ("-sS", "TCP SYN 'half-open' scan (needs root)"),
    ("--script", "Run the given NSE scripts"),
    ("-p", "Port(s) / range to scan"),
    ("-T", "Timing template 0 (paranoid) … 5 (insane)"),
    ("-oA", "Write output in all formats to <base>.{nmap,gnmap,xml}"),
];

// ── sqlmap ───────────────────────────────────────────────────────────────────

static SQLMAP_FIELDS: &[FieldSpec] = &[
    FieldSpec { key: "url", label: "Target URL", placeholder: "http://host/p?id=1", help: "-u — the URL to test", required: true },
    FieldSpec { key: "reqfile", label: "Request file", placeholder: "req.txt", help: "-r — an HTTP request saved from Burp/ZAP", required: true },
    FieldSpec { key: "level", label: "Level", placeholder: "1–5", help: "--level — tests to run (default 1)", required: false },
    FieldSpec { key: "risk", label: "Risk", placeholder: "1–3", help: "--risk — intrusiveness (default 1)", required: false },
    FieldSpec { key: "param", label: "Parameter", placeholder: "id", help: "-p — testable parameter(s)", required: false },
    FieldSpec { key: "cookie", label: "Cookie", placeholder: "PHPSESSID=…", help: "--cookie — HTTP Cookie header", required: false },
    FieldSpec { key: "proxy", label: "Proxy", placeholder: "http://127.0.0.1:8080", help: "--proxy — route through a proxy", required: false },
];

macro_rules! sqlmap_url_preset {
    ($name:literal, $aliases:expr, $purpose:literal, [$($lit:literal),*], $sharp:expr, $tags:expr) => {
        PresetSpec {
            name: $name, aliases: $aliases, purpose: $purpose,
            segments: &[Lit { text: "sqlmap" }, Flag { flag: "-u", key: "url" }, $(Lit { text: $lit },)*
                        Flag { flag: "-p", key: "param" }, Joined { prefix: "--level=", key: "level" },
                        Joined { prefix: "--risk=", key: "risk" }, Joined { prefix: "--cookie=", key: "cookie" },
                        Joined { prefix: "--proxy=", key: "proxy" }],
            fields: &["url", "param", "level", "risk", "cookie", "proxy"], sharp: $sharp, tags: $tags,
            category: "sqli",
        }
    };
}

static SQLMAP_PRESETS: &[PresetSpec] = &[
    sqlmap_url_preset!("url", &["u"], "Test a single URL for SQL injection", [], false, &[]),
    sqlmap_url_preset!("enumerate", &["enum", "dbs"], "Enumerate the databases (--dbs)", ["--dbs"], false, &[]),
    sqlmap_url_preset!("dump", &["extract"], "Dump table data — writes data out", ["--dump"], true, &["writes-data", "loud"]),
    sqlmap_url_preset!("crawl", &["spider"], "Crawl the site and test found links", ["--crawl=2"], false, &["long-running"]),
    sqlmap_url_preset!("forms", &["form"], "Parse and test forms", ["--forms"], false, &[]),
    PresetSpec {
        name: "request-file", aliases: &["r", "req"], purpose: "Test an HTTP request saved from Burp/ZAP (-r)",
        segments: &[Lit { text: "sqlmap" }, Flag { flag: "-r", key: "reqfile" },
                    Joined { prefix: "--level=", key: "level" }, Joined { prefix: "--risk=", key: "risk" }],
        fields: &["reqfile", "level", "risk"], sharp: false, tags: &[], category: "sqli",
    },
];

static SQLMAP_FLAG_HELP: &[(&str, &str)] = &[
    ("-u", "Target URL"),
    ("-r", "Load the HTTP request from a file (Burp/ZAP export)"),
    ("--dbs", "Enumerate the databases"),
    ("--tables", "Enumerate tables"),
    ("--columns", "Enumerate columns"),
    ("--dump", "Dump table entries (writes data out)"),
    ("--crawl=2", "Crawl the site to depth 2"),
    ("--forms", "Parse and test forms"),
    ("--level=", "Tests to perform 1–5 (higher = more, slower)"),
    ("--risk=", "Risk of tests 1–3 (higher = more intrusive)"),
    ("-p", "Testable parameter(s)"),
    ("--cookie=", "HTTP Cookie header"),
    ("--proxy=", "Route traffic through a proxy"),
];

// ── feroxbuster ──────────────────────────────────────────────────────────────

static FEROX_FIELDS: &[FieldSpec] = &[
    FieldSpec { key: "url", label: "Target URL", placeholder: "http://host", help: "-u — the base URL", required: true },
    FieldSpec { key: "wordlist", label: "Wordlist", placeholder: "/usr/share/seclists/…", help: "-w — wordlist path", required: true },
    FieldSpec { key: "ext", label: "Extensions", placeholder: "php,html,txt", help: "-x — file extensions to append", required: false },
    FieldSpec { key: "threads", label: "Threads", placeholder: "50", help: "-t — concurrent threads", required: false },
    FieldSpec { key: "rate", label: "Rate limit", placeholder: "req/s", help: "--rate-limit — cap requests/second", required: false },
    FieldSpec { key: "depth", label: "Depth", placeholder: "4", help: "-d — max recursion depth", required: false },
    FieldSpec { key: "status", label: "Status codes", placeholder: "200,301", help: "-s — only show these status codes", required: false },
    FieldSpec { key: "output", label: "Output file", placeholder: "ferox.txt", help: "-o — write results to a file", required: false },
];

macro_rules! ferox_preset {
    ($name:literal, $aliases:expr, $purpose:literal, [$($lit:literal),*], $tags:expr) => {
        PresetSpec {
            name: $name, aliases: $aliases, purpose: $purpose,
            segments: &[Lit { text: "feroxbuster" }, Flag { flag: "-u", key: "url" }, Flag { flag: "-w", key: "wordlist" },
                        $(Lit { text: $lit },)* Flag { flag: "-x", key: "ext" }, Flag { flag: "-t", key: "threads" },
                        Flag { flag: "--rate-limit", key: "rate" }, Flag { flag: "-d", key: "depth" },
                        Flag { flag: "-s", key: "status" }, Flag { flag: "-o", key: "output" }],
            fields: &["url", "wordlist", "ext", "threads", "rate", "depth", "status", "output"],
            sharp: false, tags: $tags, category: "content",
        }
    };
}

static FEROX_PRESETS: &[PresetSpec] = &[
    ferox_preset!("dir", &["d"], "Standard content discovery", [], &[]),
    ferox_preset!("deep", &["recursive"], "Recursive discovery with a depth limit", ["-d", "4"], &[]),
    ferox_preset!("ext", &["extensions"], "Discovery with common file extensions", ["-x", "php,html,txt"], &[]),
    ferox_preset!("fast", &["aggressive"], "High thread count (loud)", ["-t", "100"], &["loud"]),
    ferox_preset!("polite", &["slow"], "Rate-limited, low threads", ["--rate-limit", "50", "-t", "10"], &[]),
];

static FEROX_FLAG_HELP: &[(&str, &str)] = &[
    ("-u", "Target URL"),
    ("-w", "Wordlist path"),
    ("-x", "File extensions to append (e.g. php,html,txt)"),
    ("-t", "Number of concurrent threads"),
    ("--rate-limit", "Cap requests per second"),
    ("-d", "Maximum recursion depth"),
    ("-s", "Only show these HTTP status codes"),
    ("-C", "Filter out these HTTP status codes"),
    ("-o", "Write results to a file"),
    ("-n", "Disable recursion"),
];

// ── John the Ripper ──────────────────────────────────────────────────────────

static JOHN_FIELDS: &[FieldSpec] = &[
    FieldSpec { key: "hashfile", label: "Hash file", placeholder: "hashes.txt", help: "File of hashes to crack", required: true },
    FieldSpec { key: "wordlist", label: "Wordlist", placeholder: "/usr/share/seclists/…", help: "--wordlist= — dictionary", required: false },
    FieldSpec { key: "format", label: "Format", placeholder: "raw-md5, sha512crypt…", help: "--format= — hash format (Jumbo has hundreds)", required: false },
    FieldSpec { key: "mask", label: "Mask", placeholder: "?u?l?l?l?d?d", help: "--mask= — Jumbo mask (?u ?l ?d ?s)", required: true },
    FieldSpec { key: "archive", label: "File", placeholder: "secret.zip", help: "The protected file to extract a hash from", required: true },
];

static JOHN_PRESETS: &[PresetSpec] = &[
    PresetSpec {
        name: "auto", aliases: &["a"], purpose: "Autodetect — John's default single → wordlist → incremental",
        segments: &[Lit { text: "john" }, Joined { prefix: "--format=", key: "format" }, Field { key: "hashfile" }],
        fields: &["hashfile", "format"], sharp: false, tags: &[], category: "crack",
    },
    PresetSpec {
        name: "wordlist", aliases: &["dict", "w"], purpose: "Dictionary attack (+ optional rules)",
        segments: &[Lit { text: "john" }, Joined { prefix: "--wordlist=", key: "wordlist" },
                    Joined { prefix: "--format=", key: "format" }, Field { key: "hashfile" }],
        fields: &["hashfile", "wordlist", "format"], sharp: false, tags: &[], category: "crack",
    },
    PresetSpec {
        name: "single", aliases: &["s"], purpose: "Single-crack mode (login/GECOS based)",
        segments: &[Lit { text: "john" }, Lit { text: "--single" }, Joined { prefix: "--format=", key: "format" }, Field { key: "hashfile" }],
        fields: &["hashfile", "format"], sharp: false, tags: &[], category: "crack",
    },
    PresetSpec {
        name: "incremental", aliases: &["brute", "inc"], purpose: "Brute-force — tries all combinations (can run forever)",
        segments: &[Lit { text: "john" }, Lit { text: "--incremental" }, Joined { prefix: "--format=", key: "format" }, Field { key: "hashfile" }],
        fields: &["hashfile", "format"], sharp: true, tags: &["long-running"], category: "crack",
    },
    PresetSpec {
        name: "mask", aliases: &["m"], purpose: "Mask attack (Jumbo only) — ?u?l?d?s classes",
        segments: &[Lit { text: "john" }, Joined { prefix: "--mask=", key: "mask" }, Joined { prefix: "--format=", key: "format" }, Field { key: "hashfile" }],
        fields: &["hashfile", "mask", "format"], sharp: false, tags: &["jumbo-only"], category: "crack",
    },
    PresetSpec {
        name: "show", aliases: &["cracked"], purpose: "Show passwords already cracked",
        segments: &[Lit { text: "john" }, Lit { text: "--show" }, Joined { prefix: "--format=", key: "format" }, Field { key: "hashfile" }],
        fields: &["hashfile", "format"], sharp: false, tags: &[], category: "crack",
    },
    // `prepare` category — the *2john helpers extract a crackable hash from a
    // protected file (Jumbo). `sec john prepare` filters to these.
    PresetSpec { name: "zip2john", aliases: &["zip"], purpose: "Extract a hash from a ZIP archive (Jumbo)", segments: &[Lit { text: "zip2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "rar2john", aliases: &["rar"], purpose: "Extract a hash from a RAR archive (Jumbo)", segments: &[Lit { text: "rar2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "7z2john", aliases: &["7z"], purpose: "Extract a hash from a 7-Zip archive (Jumbo)", segments: &[Lit { text: "7z2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "ssh2john", aliases: &["ssh"], purpose: "Extract a hash from an SSH private key (Jumbo)", segments: &[Lit { text: "ssh2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "pdf2john", aliases: &["pdf"], purpose: "Extract a hash from a PDF (Jumbo)", segments: &[Lit { text: "pdf2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "office2john", aliases: &["office", "docx"], purpose: "Extract a hash from an MS Office file (Jumbo)", segments: &[Lit { text: "office2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "keepass2john", aliases: &["keepass", "kdbx"], purpose: "Extract a hash from a KeePass DB (Jumbo)", segments: &[Lit { text: "keepass2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
    PresetSpec { name: "gpg2john", aliases: &["gpg"], purpose: "Extract a hash from a GPG key (Jumbo)", segments: &[Lit { text: "gpg2john" }, Field { key: "archive" }], fields: &["archive"], sharp: false, tags: &["jumbo-only", "prepare"], category: "prepare" },
];

static JOHN_FLAG_HELP: &[(&str, &str)] = &[
    ("--single", "Single-crack mode (uses login names / GECOS)"),
    ("--incremental", "Incremental brute-force — all combinations (endless)"),
    ("--show", "Show already-cracked passwords from the pot file"),
    ("--wordlist=", "Dictionary file for wordlist mode"),
    ("--rules", "Apply word-mangling rules to the wordlist"),
    ("--mask=", "Mask attack (Jumbo) — ?u upper ?l lower ?d digit ?s symbol"),
    ("--format=", "Hash format (Jumbo ships hundreds — see --list=formats)"),
];

static JOHN_NOTES: &[&str] = &[
    "Two lines: Core (single/wordlist/incremental) vs Jumbo (adds mask, PRINCE, the *2john helpers, hundreds of --format values). Set your line in Settings → Security.",
    "mask, and every *2john helper, need the Jumbo build (the one shipped with Kali).",
];

// ── gobuster — extensibility smoke test (pure data, no core change) ──────────

static GOBUSTER_FIELDS: &[FieldSpec] = &[
    FieldSpec { key: "url", label: "Target URL", placeholder: "http://host", help: "-u — the base URL", required: true },
    FieldSpec { key: "wordlist", label: "Wordlist", placeholder: "/usr/share/seclists/…", help: "-w — wordlist path", required: true },
];

static GOBUSTER_PRESETS: &[PresetSpec] = &[PresetSpec {
    name: "dir", aliases: &["d"], purpose: "Directory / file brute-force",
    segments: &[Lit { text: "gobuster" }, Lit { text: "dir" }, Flag { flag: "-u", key: "url" }, Flag { flag: "-w", key: "wordlist" }],
    fields: &["url", "wordlist"], sharp: false, tags: &[], category: "content",
}];

static GOBUSTER_FLAG_HELP: &[(&str, &str)] =
    &[("dir", "Directory/file brute-force mode"), ("-u", "Target URL"), ("-w", "Wordlist path")];

// ── The catalogue ────────────────────────────────────────────────────────────

pub static CATALOG: &[ToolSpec] = &[
    ToolSpec { name: "nmap", aliases: &["scan", "portscan"], binary: "nmap", presets: NMAP_PRESETS, fields: NMAP_FIELDS, flag_help: NMAP_FLAG_HELP, notes: &["Timing/stealth flags (-T, -sS) are documented nmap options — use them honestly, not as an IDS-evasion claim."] },
    ToolSpec { name: "sqlmap", aliases: &["sqli"], binary: "sqlmap", presets: SQLMAP_PRESETS, fields: SQLMAP_FIELDS, flag_help: SQLMAP_FLAG_HELP, notes: &["--level/--risk raise coverage AND noise/intrusiveness. --dump writes target data to disk."] },
    ToolSpec { name: "feroxbuster", aliases: &["ferox", "content"], binary: "feroxbuster", presets: FEROX_PRESETS, fields: FEROX_FIELDS, flag_help: FEROX_FLAG_HELP, notes: &["Flag names have drifted between majors; verified against feroxbuster 2.x (-s = show status codes, -C = filter them out)."] },
    ToolSpec { name: "john", aliases: &["jtr", "ripper"], binary: "john", presets: JOHN_PRESETS, fields: JOHN_FIELDS, flag_help: JOHN_FLAG_HELP, notes: JOHN_NOTES },
    ToolSpec { name: "gobuster", aliases: &["gobust"], binary: "gobuster", presets: GOBUSTER_PRESETS, fields: GOBUSTER_FIELDS, flag_help: GOBUSTER_FLAG_HELP, notes: &["Added purely as a registry entry — proof that a new tool needs no code change."] },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every literal token a preset would emit, in order.
    fn literals(p: &PresetSpec) -> Vec<&'static str> {
        p.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Lit { text } => Some(*text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tool_names_and_aliases_are_globally_unique() {
        // A name/alias that collides across tools would make the frontend's
        // tool resolver ambiguous — the first hit would silently shadow the
        // other. (sec/mod.rs checks per-tool uniqueness; this is the cross-tool
        // guard.)
        let mut seen = HashSet::new();
        for t in CATALOG {
            assert!(seen.insert(t.name), "duplicate tool name {}", t.name);
            for a in t.aliases {
                assert!(seen.insert(*a), "alias {a} collides (tool {})", t.name);
            }
        }
    }

    #[test]
    fn preset_aliases_are_unique_within_a_tool_and_never_shadow_a_name() {
        for t in CATALOG {
            let mut seen = HashSet::new();
            for p in t.presets {
                assert!(seen.insert(p.name), "{}: preset name {} reused", t.name, p.name);
            }
            for p in t.presets {
                for a in p.aliases {
                    assert!(seen.insert(*a), "{}: preset alias {a} collides", t.name);
                }
            }
        }
    }

    #[test]
    fn every_preset_starts_with_a_real_binary_token() {
        // The first literal is the program that will actually run — it must be
        // a non-flag word (the *2john helpers legitimately differ from the
        // tool's own binary, so we don't force `== t.binary`).
        for t in CATALOG {
            for p in t.presets {
                let first = literals(p)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| panic!("{}::{} emits no literal", t.name, p.name));
                assert!(!first.starts_with('-'), "{}::{} starts with a flag {first}", t.name, p.name);
                assert!(!first.is_empty());
            }
        }
    }

    #[test]
    fn each_tools_own_binary_appears_as_a_leading_literal_somewhere() {
        // At least one preset must invoke the tool's declared binary directly
        // (the prepare/*2john presets are the deliberate exception).
        for t in CATALOG {
            let runs_binary = t
                .presets
                .iter()
                .any(|p| literals(p).first() == Some(&t.binary));
            assert!(runs_binary, "{}: no preset invokes its binary {}", t.name, t.binary);
        }
    }

    #[test]
    fn sharp_flagging_is_honest_in_both_directions() {
        // `sharp` is author-curated, NOT mechanically derived from the tags
        // (e.g. `nmap os`/`stealth-syn` are needs-root but intentionally not
        // sharp; `udp-top`/`vuln` are long-running but not sharp). Only two
        // directions are guaranteed:
        for t in CATALOG {
            for p in t.presets {
                // (1) A preset that WRITES TARGET DATA out must always confirm.
                if p.tags.contains(&"writes-data") {
                    assert!(p.sharp, "{}::{} writes data but is not sharp", t.name, p.name);
                }
                // (2) A sharp preset is never sharp without a reason — it must
                // carry at least one honest danger tag.
                if p.sharp {
                    assert!(!p.tags.is_empty(), "{}::{} is sharp with no danger tag", t.name, p.name);
                }
            }
        }
        // Spot-check the three canonical destructive presets.
        let sharp: HashSet<(&str, &str)> = CATALOG
            .iter()
            .flat_map(|t| t.presets.iter().filter(|p| p.sharp).map(move |p| (t.name, p.name)))
            .collect();
        for want in [("nmap", "full-tcp"), ("sqlmap", "dump"), ("john", "incremental")] {
            assert!(sharp.contains(&want), "{want:?} must be sharp");
        }
    }

    #[test]
    fn no_preset_tag_is_marketing() {
        // Tags are honest, non-marketing chips — the catalogue may only use the
        // documented vocabulary, never a "stealth"/"bypass"/"undetectable" sell.
        const ALLOWED: &[&str] =
            &["long-running", "loud", "writes-data", "needs-root", "jumbo-only", "prepare"];
        for t in CATALOG {
            for p in t.presets {
                for tag in p.tags {
                    assert!(ALLOWED.contains(tag), "{}::{} has undocumented tag {tag}", t.name, p.name);
                }
            }
        }
    }

    #[test]
    fn required_fields_are_actually_emitted_by_the_presets_that_use_them() {
        // If a preset lists a required field, the built command must contain a
        // segment that emits it — otherwise the "required" placeholder never
        // shows and the field is silently dead.
        for t in CATALOG {
            let required: HashSet<&str> =
                t.fields.iter().filter(|f| f.required).map(|f| f.key).collect();
            for p in t.presets {
                let emitted: HashSet<&str> = p
                    .segments
                    .iter()
                    .filter_map(|s| match s {
                        Segment::Field { key } | Segment::Flag { key, .. } | Segment::Joined { key, .. } => Some(*key),
                        Segment::Lit { .. } => None,
                    })
                    .collect();
                for key in p.fields {
                    if required.contains(key) {
                        assert!(emitted.contains(key), "{}::{} declares required {key} but never emits it", t.name, p.name);
                    }
                }
            }
        }
    }

    #[test]
    fn autocomplete_data_is_present_and_sane() {
        assert!(!COMMON_WORDLISTS.is_empty());
        for w in COMMON_WORDLISTS {
            assert!(w.starts_with('/'), "wordlist path {w} is not absolute");
        }
        // Both John lines are represented so the Core/Jumbo filter has entries.
        assert!(JOHN_FORMATS.iter().any(|f| !f.jumbo), "no Core formats");
        assert!(JOHN_FORMATS.iter().any(|f| f.jumbo), "no Jumbo formats");
    }
}
