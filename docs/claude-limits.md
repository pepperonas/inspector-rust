# `limits` / `quota` — Claude subscription limits

Shows the usage limits of the Claude subscription in the preview, the way
claude.ai → Settings → Usage shows them: the current session, the week across
all models, per-model weekly limits, extra usage and the weekly split by
surface. Each limit gets a bar, its percentage and when it resets — relative
("in 2 Std. 31 Min.") and absolute in Europe/Berlin.

## Source

There is no official API. Inspector Rust calls the **undocumented** endpoint
that Claude Code's own `/usage` uses:

```
GET https://api.anthropic.com/api/oauth/usage
Authorization: Bearer <Claude Code access token>
anthropic-beta: oauth-2025-04-20
```

A real response with sensitive values masked is
[`usage-api-sample.json`](./usage-api-sample.json); the Rust parser tests run
against it. The endpoint can change without notice.

## Token handling

- The token belongs to Claude Code and is read **fresh on every request**:
  macOS keychain item `Claude Code-credentials` (via `/usr/bin/security`,
  which is already on the item's access list — no prompt), falling back to
  `~/.claude/.credentials.json`; of the two, the one that expires later wins.
- **No refresh flow, nothing is written back.** A second writer could corrupt
  Claude Code's login. An expired token (or a 401/403) shows
  "Token abgelaufen — Claude Code einmal starten".
- The token is never logged, stored, put in a fixture, or sent anywhere but
  `api.anthropic.com`.

## Parsing

Defensive: every field is optional, one broken entry never drops the others.

| Source | Shown as |
|---|---|
| `limits[]` kind `session` | Aktuelle Sitzung |
| `limits[]` kind `weekly_all` | Woche · alle Modelle |
| `limits[]` kind `weekly_scoped` | Woche · `<scope.model.display_name>` |
| `limits[]` unknown kind | the raw kind, marked *unbekannt* |
| `five_hour` / `seven_day` | only when `limits[]` is missing or empty |
| other top-level objects (codename budgets like `iguana_necktie`) | not shown |
| `spend` (fallback `extra_usage`) | Zusätzliche Nutzung |
| `seven_day_breakdown.rows` | one line "Wochennutzung nach Bereich" |

## Polling

The panel asks the backend every 30 s while it is open; the backend only goes
to the network when the cached report is older than the interval (default
5 min, choosable 2–60, setting `climits.poll_minutes`). On a 429 it backs off
exponentially (doubling from the interval, honouring `Retry-After`, capped at
1 h) and keeps showing the last report marked *veraltet* with "Stand hh:mm".
Nothing polls while the panel is closed. R forces a refresh (except during a
backoff).

## Codex and Antigravity

Both are read from **local files only** — no network, no token.

- **Codex** writes its limits into every `token_count` event of
  `~/.codex/sessions/**/*.jsonl` (`rate_limits.limit_id == "codex"`,
  `primary`/`secondary` with `used_percent`, `window_minutes`, `resets_at`).
  The newest event wins; events with another `limit_id` (e.g. `premium`, null
  windows) are skipped. A window whose `resets_at` has passed shows 0 %. The
  values are as fresh as the last Codex turn — the panel says when that was.
  Parsed files are cached by path + mtime.
- **Antigravity** exposes no percentages. Its CLI logs
  (`~/.gemini/antigravity-cli/log/cli-YYYYMMDD_HHMMSS.log`) contain a line
  when a request hits the wall: `E1003 06:09:48… Individual quota reached. …
  Resets in 88h4m0s.` From the newest such line the panel shows "Kontingent
  erschöpft, wieder frei …" or "Keine aktive Sperre bekannt". Hidden when
  Antigravity isn't installed.

## Code

`core/rust-lib/src/claude_limits.rs` (parser, token read, cache/backoff —
pure parts unit-tested) · IPC `claude_limits_status`, `set_claude_limits_poll`
· `core/frontend/src/lib/claude-limits.ts` (formatting, phase) ·
`components/ClaudeLimitsPanel.tsx` · `core/rust-lib/src/agent_limits.rs` (Codex / Antigravity).

Live check (prints names and percentages only):
`cargo test -p inspector-rust-core --lib claude_limits_live -- --ignored --nocapture`
