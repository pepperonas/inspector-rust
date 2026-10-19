# GitHub integration

`github` shows the statistics of your own GitHub account: how long it exists,
followers, stars, repositories and forks, a ranking of your repositories, and
how all of it grew over time. Inspector asks GitHub for the current numbers and
keeps **local snapshots** from the day you connect. Star and fork history
reaches back further: GitHub's stargazer and fork lists carry a date per entry,
and Inspector rebuilds the curve from them back to each repository's creation
(see *Reconstructed history*). Followers have no dates on GitHub, so their
history starts with the first refresh. Nothing is estimated.

```text
github                      dashboard: profile + account age, KPI cards, star + follower charts, stars per month, top repositories
github insights             composition, star concentration, forks + repositories over time, repositories per year, fastest growing, topics, dormant
github repos [sort]         ranking — stars | forks | updated | name | growth7d | growth30d
github repo <name>          one repository: rates, history since creation, commits, releases, languages, traffic
github followers            follower history
github stars                total stars, stars by repository, top 5 over time
github trending             your repositories that gained stars in the last 7 days
github refresh              ask GitHub now
github export json|csv      to ~/Downloads; add `private` to include private repositories
github connect              sign in through the browser
```

The panel shows while you type and renders from the local cache. `refresh`,
`export` and `connect` run on Enter. With the panel focused, ←/→ switch the
chart range (7D · 30D · 90D · 1Y · 5Y · ALL); the panel opens on 1Y and keeps
your choice until the app restarts. Tab completes subcommands, sort keys
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

The browser login uses the public client id of the Inspector Rust OAuth app
(`Ov23liC708D0C6jc8iH4`, device flow enabled), built into the app since
0.210.1. A client id is not a secret, and the device flow needs no client
secret, so none is stored anywhere. For development, `IR_GITHUB_CLIENT_ID`
overrides it; a build without an id disables "Connect GitHub" and points to
the token.

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

The dashboard starts with the **profile**: name, bio, company, location and
website, and the **account age** (years and months since the account was
created, plus the date). Under it four rates: stars per year and repositories
per year of account age, the follower/following ratio and the number of public
gists.

Then four KPI cards: **Followers**, **Total Stars**, **Repositories** and
**Forks**. Each shows its 30-day change and a row of 24h / 7d / 30d / 90d / 1y
changes. Stars, forks and repositories get their changes from the
reconstructed history, so they are known right after the first refresh;
followers need snapshots.

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

**Stars per month** shows the stars gained in each of the last 12 calendar
months (UTC). A month before the known history is drawn hatched ("unknown"),
not as 0.

The history charts draw **steps**, not diagonals: a count holds until it
changes, so a line between two points would claim values that never existed.
A range starts with the value carried in from before it and ends now, so a
quiet repository still has a (flat) line. A dashed line marks where real
snapshots begin; left of it the curve is reconstructed. Each chart fills the
area under the line, marks the newest value with a dot and labels the real
minimum and maximum.

## Insights

`github insights` adds what the current repository list tells without any
history:

- **Composition:** own repositories vs. forks of others, and active / dormant /
  archived. Dormant means no push for 180 days and not archived.
- **Where the stars are:** the top repository's share, the top 3's share, and
  how many repositories hold half of all stars.
- **Total forks** and **Repositories** over time (repositories counted from
  their creation dates; deleted ones are unknown).
- **Repositories created per year**, every year from the first one.
- **Fastest growing:** stars per month since creation. Repositories younger
  than a month are left out — their rate would be noise.
- **Topics** across the repositories, and the list of **dormant**
  repositories, longest-quiet first.

## Reconstructed history

For every repository you own (forks included — their stars count in your total) with stars or forks,
Inspector reads two lists after a refresh:

- `GET /repos/{repo}/stargazers` with `Accept: application/vnd.github.star+json`
  — the date each current stargazer starred;
- `GET /repos/{repo}/forks?sort=oldest` — the date each fork was created.

Only the dates are stored (`github_events`); no names. A cumulative count of
them is the curve before the first snapshot; from the first snapshot on the
snapshots count, because they also see stars that were removed.

Rules:

- **Whole lists only.** A list is replaced in one transaction after it was
  read completely. An error, or running low on requests, drops the half-read
  list and keeps the previous one.
- **Budget:** at most 60 requests per refresh. Never-read lists first, the
  biggest first; a list that doesn't fit waits for the next refresh, and a
  list bigger than the whole budget gets a refresh of its own.
- **Rate limit:** nothing is read with fewer than 500 requests left, and a
  read stops as soon as an answer reports less.
- **Re-reading:** a list is read again only when its count changed, and at
  most every 6 hours.
- **Limits:** the curve counts today's stargazers — stars that were removed
  again are missing from the past part. GitHub lists at most 40,000 entries;
  a longer list is marked as cut.

The dashboard says how far this got ("Reconstructing history: 4 of 9 lists
read" / "History reconstructed back to 2019-03-04").

## Repository analytics

The ranking lists every repository you own, with these columns: name, stars
(with the 7-day change), forks, watchers, open issues, language and last
update. Private repositories carry a 🔒, archived ones are marked.

Sort orders:

- **Stars** (the default), **Forks**, **Recently updated** and **Name** use the
  current numbers.
- **Growth 7d** and **Growth 30d** use the stored history. Repositories without
  enough history come last, not as 0.

`github repo <name>` shows one repository:

- description, language, licence, creation date and age, size, default
  branch, homepage and topics;
- stars and forks with their 24h…1y changes;
- age, **stars per month** since creation, **fork ratio** (forks per star),
  **last push** (marked dormant after 180 days), watchers, open issues (GitHub
  counts pull requests in), stars gained in 30 days and in a year;
- charts of stars since creation, stars per month, forks, watchers and open
  issues;
- **commits per week** over 52 weeks, **releases** (count, latest tag and how
  long ago, asset downloads, average days between releases), **languages by
  code size**, and **views and clones** of the last 14 days.

The last group comes from separate endpoints, fetched when you open the
repository and cached for a day. GitHub computes commit statistics lazily —
the first request may answer "still computing", and the panel says so. Views
and clones need push access; without it those cards are left out (the "no"
is cached for a day, too).

Watchers (subscribers) come only from GitHub's per-repository endpoint. The
background refresh checks up to five repositories per refresh, each at most
once a day; a changed count becomes a snapshot, so watchers have a history.

## Historical tracking

Snapshots live in the existing `history.db`, in four tables:
`github_accounts`, `github_repositories`, `github_account_snapshots` and
`github_repository_snapshots`. Three more hold the reconstruction and the
extras: `github_events`, `github_backfill` and `github_repo_extras`.

- **Account:** at most one regular snapshot per refresh interval (1 h by
  default; 1 / 3 / 6 / 24 h in Settings). A refresh inside the interval stores
  one only when a number changed. Two identical snapshots never land in the
  same interval.
- **Repositories:** a snapshot only when stars, forks, watchers or issues
  changed, or on first sight. Fifty quiet repositories don't add fifty rows
  an hour. A list refresh carries the stored watchers count, so the watchers
  series has no gaps.

The value at any moment is the newest snapshot before it. Charts draw these
points as steps, with no interpolation; a single known point says "History
collecting…" instead of drawing a line.

Repositories are keyed by GitHub's **stable id**:

- A **renamed** repository keeps its history and appears under its new name.
- A **deleted** repository disappears from the ranking, but its history stays.
  When the repository list was cut at the page limit (3,000 repositories),
  nothing is marked deleted.
- A **private ↔ public** switch only updates the flag.

## Refresh and rate limits

- The background refresher checks every five minutes whether a refresh is
  due. The interval decides, so GitHub is asked at most once per interval.
  Opening the panel requests the same gated refresh.
- `github refresh` asks now, at most once a minute.
- After a network error the next automatic try comes after 5 minutes, not a
  whole interval later.
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
| Pure core: API types and parsing, pagination, snapshot policy, deltas, refresh gate, sorting, device-flow states, reconstruction (`cumulative`, `merge_history`, `window`, `gains_by_month`, `plan_backfill`), extras parsing | `core/rust-lib/src/github/mod.rs` |
| HTTP behind a `Transport` trait (ureq in the app, a fake in tests) | `github/client.rs` |
| Tables and snapshot recording | `github/store.rs` |
| JSON / CSV | `github/export.rs` |
| Tokens, device flow, refresh, background refresher, views | `github/service.rs` |
| Panel | `core/frontend/src/components/GithubPanel.tsx` |
| Grammar, formatting, chart helpers, insights (`ageParts`, `repoInsight`, `composition`, `concentration`, `createdByYear`, `fastestGrowing`) | `core/frontend/src/lib/github.ts` |
| Step chart paths | `core/frontend/src/lib/stats-chart.ts` (`stepPath`, `stepAreaPath`) |

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
- the CSV format and that private repositories are left out of the export;
- the reconstruction: star media type, whole-list storage, the page and rate
  budget, the hand-over from events to snapshots, the carried-in window value;
- a cut repository list never marks repositories deleted;
- the growth chart ranks by growth, not size;
- extras caching, "still computing" and a missing permission.
