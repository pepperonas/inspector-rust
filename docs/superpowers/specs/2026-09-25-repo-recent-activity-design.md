# Repo-Aktivität „24 h / 7 Tage" (inkl. GitHub-Pushes, PRs, Issues)

Stand: 2026-09-25 · Status: Entwurf, im Chat abgestimmt · baut auf v0.184.0 (`repo`-Panel) auf

## Ziel

Im `repo`-Panel sieht man auf einen Blick, was in den **letzten 24 Stunden** und den
**letzten 7 Tagen** passiert ist — Commits, Code-Zeilen, Dateien, Mitwirkende, Tags,
und bei GitHub-Repos zusätzlich Pushes, Pull Requests und Issues — jeweils mit dem
Vergleich zur gleich langen Vorperiode.

## Vom Nutzer entschieden

| Frage | Entscheidung |
|---|---|
| Zeitbasis | **rollierend** ab jetzt: 24 h und 7 × 24 h |
| Pushes | ja, per **GitHub-API**, dazu PRs geöffnet/gemergt und Issues geöffnet/geschlossen |
| Vergleich | ja, **↑/↓ mit Differenz** zur Vorperiode |

## Fenster

Für `now` (Wanduhr beim Analysieren, Unix-Sekunden) vier halboffene Intervalle:

| Name | Intervall |
|---|---|
| `day` | `(now − 24 h, now]` |
| `day_prev` | `(now − 48 h, now − 24 h]` |
| `week` | `(now − 7 d, now]` |
| `week_prev` | `(now − 14 d, now − 7 d]` |

Ein Ereignis genau auf einer Grenze gehört zum jüngeren Fenster **nicht** (halboffen
links): `t > start && t <= end`.

## 1. Git-Werte (`repo_activity.rs`, rein)

- Zeitpunkt eines Commits = **Commit-Datum** (`%cI`), nicht Autor-Datum — ein heute
  per Rebase/Cherry-Pick neu geschriebener alter Commit ist heute passiert. Dafür
  bekommt `Commit` ein Feld `committed: Option<i64>` (Unix-Sekunden), geparst aus
  einem zusätzlichen Feld im bestehenden `git log`-Format. Die Zeiträume des Panels
  (30 T … Gesamt) bleiben unverändert beim Autor-Datum.
- Je Fenster (`ActivityCounts`): `commits`, `insertions`, `deletions`,
  `files` (eindeutige Pfade), `authors` (eindeutige Autoren), `tags` (Tags, deren
  Erstellungsdatum ins Fenster fällt).
- Tag-Daten: `git for-each-ref refs/tags --format=%(creatordate:unix)` im Repo bzw.
  Cache (lightweight Tags: Datum des Commits, annotierte: Datum des Tags).
- Grundlage wie im Rest des Panels: Haupt-Branch (`origin/HEAD` bzw. `HEAD` lokal),
  `--no-merges`. Das Panel sagt in einer Zeile: „Git: Haupt-Branch ohne Merges ·
  Pushes: alle Branches".
- Ergebnis `RecentActivity { now: i64, day, day_prev, week, week_prev }` hängt als
  Feld `recent` an `RepoAnalysis` — kein zusätzlicher Aufruf.

## 2. GitHub-Werte (`github_api.rs`, neuer IPC `repo_github_activity`)

Nur für GitHub-Repos (`RepoAnalysis.github` gesetzt). Endpunkte (je max. 3 Seiten à 100):

| Wert | Endpunkt | Zählung |
|---|---|---|
| Pushes | `GET /repos/{o}/{r}/events` | `type == "PushEvent"`, nach `created_at` |
| PRs geöffnet | `GET /repos/{o}/{r}/pulls?state=all&sort=updated&direction=desc` | `created_at` im Fenster |
| PRs gemergt | (dieselbe Antwort) | `merged_at` im Fenster |
| Issues geöffnet | `GET /repos/{o}/{r}/issues?state=all&since={now−14d}` | ohne Einträge mit `pull_request`; `created_at` |
| Issues geschlossen | (dieselbe Antwort) | `closed_at` im Fenster |

- Pulls: Seitenabruf endet, sobald `updated_at` älter als `now − 14 d` ist.
- **Events-Grenze:** Die API liefert höchstens 300 Ereignisse (und nur 90 Tage). Endet
  die dritte Seite noch innerhalb des 14-Tage-Horizonts, ist die Push-Zahl
  **unvollständig** → Feld `pushes_capped: true`, UI zeigt „≥ n".
- Seit 2025 enthält ein `PushEvent` keine Commit-Anzahl mehr (nur `ref`, `before`,
  `head`) — gezählt wird die Zahl der Pushes, nicht der gepushten Commits.
- Auth: Token aus `repo_clone::github_token()`; Header `Authorization: Bearer …`,
  **nie** in der URL; `retry_without_token` bei 401/403 mit Token. Header
  `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`,
  `User-Agent: inspector-rust`. Zeitlimit 8 s je Anfrage.
- Fehler: `github.rate_limit` (403/429 mit `x-ratelimit-remaining: 0`),
  `github.not_found` (404 — privat ohne Zugriff oder falscher Name),
  `github.network`, `github.http` — im Panel als kleiner Hinweis im GitHub-Block; die
  Git-Werte bleiben unberührt.
- Ergebnis `GithubActivity { day, day_prev, week, week_prev: GithubCounts,
  pushes_capped: bool }` mit `GithubCounts { pushes, prs_opened, prs_merged,
  issues_opened, issues_closed }`.
- Pure Parser (`count_events`, `count_pulls`, `count_issues`) arbeiten auf
  `serde_json::Value` und `now`; die HTTP-Schicht ist dünn.

## 3. Oberfläche und Export

- Karte **„Aktivität"** ganz oben im Panel (über den Zeitraum-Chips), Tabelle mit den
  Spalten **24 h** und **7 Tage**.
- Git-Zeilen: Commits · Zeilen +/− · netto · Dateien · Mitwirkende · Tags.
- GitHub-Zeilen: Pushes · PRs geöffnet · PRs gemergt · Issues geöffnet · Issues
  geschlossen.
- Neben jeder Zahl die Differenz zur Vorperiode: `↑ 3` / `↓ 2` / `±0`, **neutral**
  eingefärbt (gedämpft), weil weniger nicht automatisch schlechter ist; ein Tooltip
  nennt den Wert der Vorperiode.
- GitHub-Block: Ladezustand während des Abrufs; bei lokalen Repos ausgeblendet; bei
  Fehler eine Zeile mit dem Hinweis (Rate-Limit → Token/`gh auth login`).
- Der HTML/PDF-Export beginnt mit derselben Tabelle (GitHub-Werte, sofern geladen:
  das Panel reicht sie mit `repo_export` durch; Feld optional, fehlt → Block entfällt).
- „Neu analysieren" lädt beide Teile neu (neues `now`).

## 4. Fehlerfälle

- Kein Commit im Fenster → 0 (nicht ausgeblendet).
- Repo ohne Tags → Tags 0.
- Commit ohne parsebares Commit-Datum → zählt in keinem Fenster.
- API nicht erreichbar / Limit / 404 → Hinweis im GitHub-Block, Rest bleibt.

## 5. Tests

- Rust rein: Fenster-Grenzen (genau auf der Grenze), Vorperiode, Rebase-Fall
  (altes Autor-Datum, neues Commit-Datum → zählt heute), eindeutige Dateien/Autoren,
  Tags im Fenster.
- API-Parser gegen **aufgezeichnete echte Antworten** (Fixtures, anonymisiert auf
  Typ/Zeitstempel/Felder), Cap-Erkennung, PRs aus `issues` herausgefiltert.
- Token nie in der URL (URL-Builder rein, geprüft).
- Panel: Karte vor den Zeitraum-Chips, Differenz-Darstellung, Ladezustand,
  Fehlerhinweis, kein GitHub-Block bei lokalem Repo.
- Mutationsproben je Kernregel.

## 6. Doku

CommandDoc `repo`, `features.txt`, `CHANGELOG.md`, `CLAUDE.md`, `docs/repo.md`,
README-Matrix über `gen-docs`, Version 0.185.0.
