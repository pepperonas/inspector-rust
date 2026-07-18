# `sec` — guided pentest-command builders (v0.84.271)

Assemble syntactically correct command lines for four standard tools — **nmap ·
sqlmap · feroxbuster · John the Ripper** — from presets + your target, with a
plain-English cheat-sheet for every flag. **Inspector Rust never runs the
tools.** It builds text and, only when you explicitly ask, hands it off to your
terminal; the tool then runs in your own shell, with your environment, logging
and sudo prompt.

> **Authorization.** Use only against systems you have **written permission** to
> test. This is a productivity / cheat-sheet tool for authorized audits, not an
> attack automator. A one-time reminder appears on first use; an optional scope
> note (Settings → Security) shows in the preview header.

## Grammar

```
sec                                → 4-tool overview
sec nmap   | nmap                  → nmap preset list
sec sqlmap | sqlmap                → sqlmap preset list
sec ferox  | feroxbuster | ferox   → feroxbuster preset list
sec john   | john                  → John preset list
<tool> <preset> <target> [--k v]   → the built command
sec john prepare                   → the *2john hash-extraction helpers
```

- Argument order is irrelevant: `nmap 10.0.0.5 service --ports 80` ==
  `nmap service --ports 80 10.0.0.5`. Bare tokens fill the preset's required
  fields in order; `--key=value` / `--key value` sets any field.
- The tool keywords (`nmap`, …) work directly for muscle-memory. A bare tool
  keyword followed by prose that isn't a preset — `nmap output parsen` — is
  **not** treated as a command; your history search wins.
- **Enter** copies the built command to the clipboard + pastes it into the
  previously-focused app (+ a history entry, unless you turn that off). It
  **never runs** anything.
- **⌘/Ctrl+Enter** hands off: opens your terminal (macOS) with the command
  **inserted but not submitted** by default — you review and press Enter
  yourself. Sharp presets confirm first (see below).

## Execution model — what the app does and doesn't do

| | |
|---|---|
| Builds the command string | ✅ frontend-side, deterministic, shell-quoted |
| Copies to clipboard / pastes | ✅ (Enter) — same as every power command |
| Opens your terminal with it | ✅ (⌘⏎, opt-in, macOS) via `osascript` |
| Runs nmap/sqlmap/… as a subprocess | ❌ never — no `Command::new("nmap")` |
| Captures tool output in-app | ❌ never |
| Makes its own network requests | ❌ never — no telemetry, no origin |
| Bundles the tools or wordlists | ❌ never — the tools are installed separately |

The **only** process the `sec` module spawns is `osascript` opening a terminal.
Everything the tool does happens in your shell, under your responsibility.

**Auto-submit** is off by default (Settings → Security → "Auto-press Enter"). A
**sharp** preset — `nmap full-tcp`, `sqlmap dump`, `nmap -T5`, `john
incremental` — always shows a native confirmation before the hand-off,
regardless of the auto-enter setting (not toggleable off).

## Shell safety

Every user-supplied value is quoted for POSIX **sh/bash** (`posixQuote`,
shlex.quote-style): a value made of safe characters passes through unquoted
(`10.0.0.5`); anything with a space, `;`, `&`, `$`, `` ` ``, `'`, `"`, `?` or
`*` is single-quoted (`'a; rm -rf /'`), with embedded `'` escaped as `'\''`. A
malicious target can never break out of its single argument — on the clipboard
**and** the hand-off path. **This targets sh/bash**; on Windows use WSL/git-bash
(the hand-off is macOS-only anyway; elsewhere the command is clipboard-only).

## Preset catalogues

Every flag below is explained in the preview's cheat-sheet table. Conservative,
non-destructive presets come first; sharp ones are marked ⚠.

### nmap — fields: target · ports (`-p`) · timing (`-T`) · output (`-oA`)
| Preset | Command | |
|---|---|---|
| quick | `nmap -F <target>` | fast scan, top 100 ports |
| top | `nmap -sV <target>` | version detection, top 1000 |
| service | `nmap -sV -sC <target>` | version + default NSE scripts |
| full-tcp ⚠ | `nmap -p- <target>` | all 65535 TCP ports (long-running) |
| udp-top | `nmap -sU <target>` | UDP top ports (slow) |
| os | `nmap -O <target>` | OS detection (needs root) |
| ping-sweep | `nmap -sn <target>` | host discovery only |
| vuln | `nmap --script vuln <target>` | 'vuln' NSE scripts (loud) |
| stealth-syn | `nmap -sS <target>` | SYN half-open scan (needs root) |

Timing/stealth flags are documented nmap options with honest descriptions —
never sold as "IDS evasion".

### sqlmap — fields: url (`-u`) · request file (`-r`) · `-p` · `--level` · `--risk` · `--cookie` · `--proxy`
| Preset | Command | |
|---|---|---|
| url | `sqlmap -u <url>` | test one URL |
| enumerate | `sqlmap -u <url> --dbs` | enumerate databases |
| dump ⚠ | `sqlmap -u <url> --dump` | dump table data (writes data out) |
| crawl | `sqlmap -u <url> --crawl=2` | crawl + test found links |
| forms | `sqlmap -u <url> --forms` | parse and test forms |
| request-file | `sqlmap -r <reqfile>` | a Burp/ZAP-saved request |

`--dump-all` is deliberately **not** a preset.

### feroxbuster — fields: url (`-u`) · wordlist (`-w`) · `-x` · `-t` · `--rate-limit` · `-d` · `-s` · `-o`
| Preset | Command | |
|---|---|---|
| dir | `feroxbuster -u <url> -w <wordlist>` | standard discovery |
| deep | `… -d 4` | recursive, depth 4 |
| ext | `… -x php,html,txt` | common extensions |
| fast | `… -t 100` | high threads (loud) |
| polite | `… --rate-limit 50 -t 10` | rate-limited |

Verified against feroxbuster 2.x: `-s`/`--status-codes` shows given codes,
`-C`/`--filter-status` filters them out (the flags have drifted across majors).

### John the Ripper — fields: hash file · wordlist · `--format` · mask
| Preset | Command | line |
|---|---|---|
| auto | `john <hashfile>` | both |
| wordlist | `john --wordlist=<wl> <hashfile>` | both |
| single | `john --single <hashfile>` | both |
| incremental ⚠ | `john --incremental <hashfile>` | both (endless) |
| mask | `john --mask=<mask> <hashfile>` | Jumbo |
| show | `john --show <hashfile>` | both |

`sec john prepare` lists the **\*2john** helpers that extract a crackable hash
from a protected file (Jumbo): `zip2john · rar2john · 7z2john · ssh2john ·
pdf2john · office2john · keepass2john · gpg2john`. e.g. `zip2john secret.zip`.

**John has two lines** — *Core* (single/wordlist/incremental) vs *Jumbo* (adds
mask, PRINCE, hundreds of `--format` values and the \*2john helpers, the build
Kali ships). Set yours in Settings → Security; Core hides the Jumbo-only presets.

## Settings → Security

Scope note · default wordlist · default output dir · default timing/threads/rate
(empty = not injected) · John line (Jumbo/Core) · preferred terminal · auto-enter
(off) · save-to-history · sharp-confirm (locked on). Persisted; a
`sec-defaults-changed` event refreshes the popup without a restart.

**Wordlist autocomplete + existence check.** The default-wordlist field
autocompletes against common Kali/SecLists paths and shows a live **✓ found /
✗ not found** indicator (`sec_path_exists`, read-only). The preview warns if a
command's wordlist isn't present on this machine. **John `--format`** offers the
verified format names as a reference in the preview, filtered by your Core/Jumbo
line (Core hides the Jumbo-only formats).

## Extending the registry — a tool is data, not code

The registry (`core/rust-lib/src/sec/registry.rs`) is the single source of
truth. Adding a tool is one `ToolSpec` — no parser, builder or UI change.
**gobuster** is in the catalogue purely as a ~15-line registry entry, as proof:

```rust
static GOBUSTER_PRESETS: &[PresetSpec] = &[PresetSpec {
    name: "dir", aliases: &["d"], purpose: "Directory / file brute-force",
    segments: &[Lit { text: "gobuster" }, Lit { text: "dir" },
                Flag { flag: "-u", key: "url" }, Flag { flag: "-w", key: "wordlist" }],
    fields: &["url", "wordlist"], sharp: false, tags: &[], category: "content",
}];
// + a ToolSpec { name: "gobuster", …, presets: GOBUSTER_PRESETS, fields, flag_help }
```

A registry-consistency test enforces that every preset references defined fields
and that every flag it uses has a plain-English explanation (no cheat-sheet
gaps). To add metasploit / hydra / nuclei later: add a `ToolSpec`, done.

## Caveats

- **Terminal hand-off is macOS-only** (iTerm2 → Terminal.app via `osascript`).
  Windows/Linux fall back to clipboard-only (shown in the preview). Auto-enter
  uses System Events `keystroke`, which needs Accessibility (already granted for
  paste/expander).
- **Flags drift.** feroxbuster changes flag names across majors; John's modes
  and formats depend on the Core/Jumbo line. The presets are verified against
  current sources, but check `--help` if your installed version differs.
- **No hard-coded targets, no bundled wordlists, no payloads.** The builder
  assembles tool syntax; the attack logic lives in the external, independently
  distributed tools — not in Inspector Rust.
