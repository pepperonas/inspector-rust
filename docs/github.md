# GitHub integration

`github` shows the statistics of your own GitHub account: followers, stars,
repositories and forks, a ranking of your repositories, and how all of it grew
over time. Inspector asks GitHub for the current numbers and keeps **local
snapshots**; the history starts the day you connect. Nothing is estimated or
filled in before that.

```text
github                      dashboard: KPI cards, follower + star charts, top repositories
github repos [sort]         ranking — stars | forks | updated | name | growth7d | growth30d
github repo <name>          one repository with its star / fork / issue history
github followers            follower history
github stars                total stars, stars by repository, top 5 over time
github trending             your repositories that gained stars in the last 7 days
github refresh              ask GitHub now
github export json|csv      to ~/Downloads; add `private` to include private repositories
github connect              sign in through the browser
```

The panel shows while you type and renders from the local cache. `refresh`,
`export` and `connect` run on Enter. With the panel focused, ←/→ switch the
chart range (7D · 30D · 90D · 1Y · ALL). Tab completes subcommands, sort keys
and your repository names.

## Authentication

Three ways, tried in this order:

1. **Sign in with GitHub** (recommended): the OAuth device flow. Inspector
   shows a short code, opens `github.com/login/device`, and you confirm there.
   No password passes through Inspector, and the flow needs no client secret.
2. **Personal access token** — Settings → GitHub → Advanced. The same keychain
   entry the `repo` command uses for private clones.
3. **GitHub CLI** — if `gh` is installed and logged in, its token is used.

The panel and Settings show which one is active ("Connected as @login" plus
the source). **Disconnect** removes the browser login. A token or `gh` login
still connects, and the status says so. The local history stays: it is your
data. `gh auth logout` ends a CLI login.

The browser login needs the public client id of the Inspector Rust OAuth app.
As long as a build has none, "Connect GitHub" is disabled and the panel points
to the token. For development, `IR_GITHUB_CLIENT_ID` sets one.

## Permissions

Inspector only reads. By default it asks for **no scope**, which gives
public data: profile, followers and public repositories.

Private repositories are opt-in (Settings → GitHub → Private repositories).
OAuth apps have no read-only scope for private repositories. The only one is
`repo`, which GitHub grants with read *and* write access, so the setting
says so plainly. Inspector never writes. The least-privilege way is a
fine-grained personal access token with **Metadata: read** for the
repositories you choose.

## Secrets

Tokens live only in the macOS keychain (service `io.celox.inspector-rust`,
accounts `github-oauth-v1` and `github-token-v1`). Tokens are never:

- written to `settings` or any file,
- returned to the UI,
- logged, or included in an error, export or crash report.

Errors travel as short sentinels such as `github.unauthorized`, and the UI
turns them into text. Tests pin that a token appears only in the
`Authorization` header and nowhere in an error or export.

## Statistics

The dashboard shows four KPI cards: **Followers**, **Total Stars**,
**Repositories** and **Forks**. Each shows its 30-day change and a row of
24h / 7d / 30d / 90d changes.

A change is computed only from real snapshots. When the history doesn't reach
back far enough, the change shows as `—` (tooltip "Insufficient history"),
never as 0. A brand-new connection says **History collecting…**.

Total stars and forks are summed over the repositories you own, forks
included. A fork's stars are yours too.

Below the KPIs, four charts need **no history** — they are drawn from the
current numbers, so they appear as soon as you connect:

- **Languages:** a donut of your repositories by language, switchable to
  weigh by stars. Repositories without a language show as "No language";
  slices under 4 % fold into "Other".
- **Last push:** a column per month (12 months) counting the repositories
  whose last push falls into it. A repository without a push date uses its
  update date. The current month is green.
- **Stars by repository** and **Forks by repository:** the top 8 as bars.
  A repository with 0 is left out, not drawn as an empty bar. Click a bar to
  open the repository.

The line charts (followers, total stars, per repository) fill the area under
the line, mark the newest value with a dot and label the real minimum and
maximum of the shown range.

## Repository analytics

The ranking lists every repository you own, with these columns: name, stars
(with the 7-day change), forks, watchers, open issues, language and last
update. Private repositories carry a 🔒, archived ones are marked.

Sort orders:

- **Stars** (the default), **Forks**, **Recently updated** and **Name** use the
  current numbers.
- **Growth 7d** and **Growth 30d** use the stored history. Repositories without
  enough history come last, not as 0.

`github repo <name>` shows one repository: description, language, licence,
when Inspector first saw it, stars and forks with their changes, watchers,
open issues, and charts of stars, forks and issues.

Watchers (subscribers) come only from GitHub's per-repository endpoint, so
the value is fetched once, when you open a repository.

## Historical tracking

Snapshots live in the existing `history.db`, in four tables:
`github_accounts`, `github_repositories`, `github_account_snapshots` and
`github_repository_snapshots`.

- **Account:** at most one regular snapshot per refresh interval (1 h by
  default; 1 / 3 / 6 / 24 h in Settings). A refresh inside the interval stores
  one only when a number changed. Two identical snapshots never land in the
  same interval.
- **Repositories:** a snapshot only when stars, forks, watchers or issues
  changed, or on first sight. Fifty quiet repositories don't add fifty rows
  an hour.

The value at any moment is the newest snapshot before it. Charts draw these
points as they are, with no interpolation. A range shows only the points
inside it, and a single point says "History collecting…" instead of
drawing a line.

Repositories are keyed by GitHub's **stable id**:

- A **renamed** repository keeps its history and appears under its new name.
- A **deleted** repository disappears from the ranking, but its history stays.
- A **private ↔ public** switch only updates the flag.

## Refresh and rate limits

- The background refresher checks every five minutes whether a refresh is
  due. The interval decides, so GitHub is asked at most once per interval.
  Opening the panel requests the same gated refresh.
- `github refresh` asks now, at most once a minute.
- Every response's `x-ratelimit-*` headers are stored. When the limit is used
  up, nothing is sent until the reset ("Rate limit reached · Next refresh
  available: 14:05").
- Offline, the panel keeps the cached data ("Offline · Showing cached GitHub
  data · Last updated 4h ago").
- A revoked token shows "GitHub rejected the token". The cached data stays
  visible.

Automatic refresh can be switched off in Settings. A token is needed for any
request; without one, nothing runs.

## Export

`github export json` or `github export csv` writes
`github-<login>-<date>.json|csv` to `~/Downloads` and never overwrites an
existing file.

**JSON** contains:

- the account, the repositories and both histories, with UTC timestamps;
- `private_repositories_included`;
- `private_repositories_left_out`, which says whether anything was held back.

**CSV** contains the repository snapshots:

```text
timestamp,repository,stars,forks,watchers,open_issues
2026-10-01T12:00:00Z,pepperonas/inspector-rust,112,15,8,3
```

Private repositories are left out unless you add `private`
(`github export csv private`). Cells that a spreadsheet would read as a
formula are defused.

## Local storage

Everything above stays on this Mac, in `history.db`. Nothing is sent
anywhere but `api.github.com` and `github.com` (login), and there is no
telemetry. The history is not deleted when you disconnect.

## Architecture

| Piece | File |
|---|---|
| Pure core: API types and parsing, pagination, snapshot policy, deltas, refresh gate, sorting, device-flow states | `core/rust-lib/src/github/mod.rs` |
| HTTP behind a `Transport` trait (ureq in the app, a fake in tests) | `github/client.rs` |
| Tables and snapshot recording | `github/store.rs` |
| JSON / CSV | `github/export.rs` |
| Tokens, device flow, refresh, background refresher, views | `github/service.rs` |
| Panel | `core/frontend/src/components/GithubPanel.tsx` |
| Grammar, formatting, chart helpers | `core/frontend/src/lib/github.ts` |

None of the tests touches the network. The client runs against a fake
transport, and the store and service against an in-memory database. The
following are pinned:

- pagination across pages;
- rate-limit handling;
- that the token appears only in the header;
- the snapshot policy and deduplication;
- deltas with missing history;
- that a rename keeps its history;
- sorting with unknown growth last;
- the CSV format and that private repositories are left out of the export.

The view types can take further metrics (traffic, releases) without a schema
change to the existing tables.
