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
| other filled top-level objects with `utilization` (codename budgets) | raw field name, marked *unbekannt*, with dollars if present |
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

## Code

`core/rust-lib/src/claude_limits.rs` (parser, token read, cache/backoff —
pure parts unit-tested) · IPC `claude_limits_status`, `set_claude_limits_poll`
· `core/frontend/src/lib/claude-limits.ts` (formatting, phase) ·
`components/ClaudeLimitsPanel.tsx`.

Live check (prints names and percentages only):
`cargo test -p inspector-rust-core --lib claude_limits_live -- --ignored --nocapture`
