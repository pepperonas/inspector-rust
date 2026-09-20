# E-mail deliverability check — `mailcheck`

`mailcheck <email>` (aliases `emailcheck`, `mailverify`, `mxcheck`) runs layered,
**honest** deliverability checks on an address and shows each stage in the
preview. It never claims a mailbox "exists" — e-mail verification has hard
limits, and the panel is built to respect them.

## Why it can't just say "valid"

Two facts govern everything here:

1. **Most mail servers deliberately prevent recipient enumeration.** They accept
   every `RCPT TO` (so acceptance proves nothing about a specific mailbox), or
   reject probes outright, or greylist them. A server that accepts all
   recipients is a *catch-all*.
2. **Outbound port 25 is routinely blocked** on ISP/residential/cloud networks.
   When it is, the SMTP stage simply can't connect — and that's reported as
   such, not as a failure of the address.

So the mailbox verdict is frequently, and correctly, **Inconclusive**. That is
the honest answer, not a bug.

## The stages

| Stage | What it does |
|---|---|
| **Syntax** | Robust-but-pragmatic validation (not full RFC 5322 — that accepts things no real address uses). Rejects missing/multiple `@`, empty or over-long local/domain, whitespace, leading/trailing/double dots, a domain without a dot or with bad labels. |
| **Domain** | Extracts and normalises the domain. |
| **DNS / MX** | A minimal **DNS-over-UDP** query (to `1.1.1.1`, then `8.8.8.8`) for the domain's MX records, listed by preference. With no MX but an A/AAAA record, the A record is used as an **implicit mail exchanger** (RFC 5321 §5.1). No MX and no address record → the domain cannot receive mail. |
| **SMTP / Mailbox** | Where port 25 is reachable, a **non-invasive** probe: connect, `EHLO`, `MAIL FROM:<>` (the null/bounce sender), `RCPT TO:<target>`, then `QUIT`. It **never sends a mail, never authenticates, and issues no aggressive probes.** |
| **Catch-all** | One extra `RCPT TO:<random@domain>` **in the same session** (not a new connection). If the random address is accepted too, the server is catch-all and the real acceptance means nothing; if it's rejected while the real one is accepted, the mailbox is genuinely likely valid. |
| **Disposable** | Flags known disposable/throwaway domains from a curated (non-exhaustive) list. |

### RCPT reply → mailbox verdict

- `250` / `251` → **Likely valid** (⚠️ may be catch-all)
- `550` / `551` / `553` → **Invalid** (no such user)
- `552` / `554` → **Unlikely** (rejected)
- `450` / `451` / `452` → **Inconclusive** (greylist / temporary)
- anything else / no connection → **Inconclusive**

## Overall status

Derived deterministically from the signals actually obtained:

- **INVALID** — syntax failed (nothing else attempted).
- **UNDELIVERABLE** — the domain has no MX/A record, or the mailbox was hard-rejected.
- **DELIVERABLE** — the mailbox was accepted and the server is *not* catch-all.
- **DELIVERABLE LIKELY** — the domain can receive mail (MX/A present) but the mailbox is unverifiable (the common case).
- **RISKY** — deliverable-ish but flagged: a disposable domain, or a catch-all server.

## Architecture

All network work lives in the Rust backend (`core/rust-lib/src/mailcheck.rs`) and
runs on `spawn_blocking`, so the UI never blocks. The command is `mailcheck_run`;
the panel is `MailCheckPanel.tsx`, driven by pure display helpers in
`lib/mailcheck.ts`.

The pure logic (syntax, domain, disposable, RCPT classification, overall
derivation, DNS packet build/parse, SMTP reply parsing) is split from the impure
sockets and unit-tested. The **DNS response parser is defensive** — name
decompression is loop-guarded, wrong transaction IDs are rejected, and a
fuzz test drives random buffers through it (it never panics on malformed input;
capture/response bytes are untrusted).

Timeouts are short and bounded per stage (DNS 3 s, SMTP connect 5 s, SMTP I/O
6 s), and at most two MX hosts are tried — the check can't hang.

## Security & privacy

- **No mail is ever sent**, no authentication is attempted, no aggressive probing.
- The catch-all probe is a single extra `RCPT` in the same connection — not a
  fan-out of random-address probes.
- No credentials are involved (DNS/SMTP need none), so none can be logged or leaked.
- Nothing is stored or transmitted beyond the DNS/SMTP lookups themselves; the
  result is transient.

## Limits

- The disposable list is a curated heads-up, not exhaustive (the space is endless).
- CoreBluetooth-style hardware isn't involved — but the SMTP result depends on
  outbound port 25, which many networks block; treat "Inconclusive" as normal.
- `mailcheck` does not check whether the domain's mail is *configured well*
  (SPF/DKIM/DMARC) — it checks whether mail can be delivered at all.
