# repo2viz-Integration: GitHub-URL → Statistik, Export, Klonen

Stand: 2026-09-25 · Status: Entwurf, vom Nutzer im Chat abgestimmt · Vorbild: [pepperonas/repo2viz](https://github.com/pepperonas/repo2viz)

## Ziel

Wer eine GitHub-Repo-URL in die Suchleiste tippt oder einfügt, bekommt in der Vorschau die
Statistik des Repos, kann sie als HTML oder PDF exportieren und das Repo mit einem Klick in
einen festen Ordner klonen.

## Was schon existiert (wird erweitert, nicht ersetzt)

- `repo <url|pfad>` / `export <url|pfad>` — `repo_stats.rs` (`parse_git_log`, `analyze_local`,
  `analyze_remote`, `build_html`), `commands.rs` (`resolve_repo_target`, `repo_analyze`,
  `repo_export`), `RepoPanel.tsx` (Vorschau, `ExportRow` html/pdf, ⌘E/⌘P).
- Schutz gegen Argument-Injection: URL mit führendem `-` wird abgewiesen, `--` vor der URL,
  `current_dir` statt `-C`.

## Vom Nutzer entschieden

| Frage | Entscheidung |
|---|---|
| Klon-Ziel | fester Ordner, in Settings einstellbar |
| Auslösung | Zeile erscheint sofort, Analyse startet erst mit Enter |
| Klon wiederverwenden | ja, Analyse-Klon wird zum Klon |
| Belegter Zielordner | neuer Ordner `repo (2)`, `repo (3)`, … hochzählen |
| Extras | Zeitraum-Umschaltung, Heatmap + Kalender, Hotspots & Bus-Faktor, private Repos (Token) |
| Nicht übernommen | Azure DevOps, PO-Dashboard, DORA, Monte-Carlo, Anonymisierung |

## 1. URL-Erkennung

Reine Funktion `parseRepoUrl(text) -> { host, owner, repo, cloneUrl } | null`, identisch in
`lib/repo-url.ts` und `repo_url.rs`; beide Seiten prüfen dieselbe Fall-Tabelle (Tests
gespiegelt, damit sie nicht auseinanderlaufen).

- Treffer: `https://github.com/o/r`, `http://…`, `github.com/o/r` (ohne Schema), `www.github.com/o/r`,
  Endung `.git`, abschließender `/`, `/tree/<ref>/…` und `/blob/<ref>/…` (gekürzt auf das Repo),
  `git@github.com:o/r.git`.
- Kein Treffer: `/issues`, `/pull`, `/actions`, `/releases`, `/wiki`, `/settings`, Gists,
  reine Nutzer-/Org-Profile (`github.com/o`), GitHub-Systempfade (`github.com/features/…`,
  `/orgs/`, `/marketplace`, …), fremde Hosts.
- Owner/Repo werden gegen die GitHub-Zeichenregeln geprüft (`[A-Za-z0-9-]` bzw. `[A-Za-z0-9._-]`);
  alles andere → `null`. Damit kann aus der URL nie eine Option oder ein Pfad-Ausbruch werden.

Einbindung in `App.tsx`: eigene `ListEntry`-Art `repo-url` („Repo analysieren · o/r"), gespliced
**über** `appEntry` (Invariante: Befehle schlagen Apps), rote Befehls-Optik (`CUSTOM_COMMAND_KINDS`).
Erscheint, wenn die Suchanfrage genau eine Repo-URL ist, und in der Vorschau eines Clips, der
eine enthält (Muster wie `social`). Enter öffnet das `repo`-Panel mit dieser URL; getippt wird
nichts ausgelöst.

## 2. Klon-Cache — gemeinsame Basis von Analyse und Klonen

- Ort: `<Cache-Dir>/InspectorRust/repos/<owner>/<repo>` (macOS `~/Library/Caches/…`).
- Analyse einer URL: existiert der Cache-Eintrag → `git fetch --prune`; sonst
  `git clone --no-checkout -- <url> <cache>`. Danach wie bisher `git log` im Cache.
  Vorteil gegenüber heute (bare nach temp, danach löschen): zweite Analyse und Klonen kosten
  keine volle Übertragung.
- Aufräumen: höchstens 5 Repos oder 2 GB; ältester (mtime des Eintrags) fliegt zuerst.
  Entscheidung ist die reine, getestete Funktion `cache_evictions(entries, max_n, max_bytes)`.
  Läuft nach jeder Analyse.
- Fortschritt: `git clone --progress` schreibt nach stderr; ein Parser (`parse_git_progress`,
  rein, getestet gegen echte Git-Ausgabe) liefert Phase + Prozent als Event
  `repo-progress {phase, percent}`.

## 3. Klonen

- Zielordner: Setting `repo.clone_dir`; Vorgabe `~/claude`, falls vorhanden, sonst `~/Downloads`.
  Tilde wird über `path_arg::expand_user` aufgelöst. Settings-Sektion „Repositories“
  (id `repos`, auch in `settings-sections.ts`).
- Ordnername: `<repo>`; ist er belegt, `<repo> (2)`, `<repo> (3)`, … — reine Funktion
  `free_dir_name(base, exists)` mit Tests (Lücken werden nicht aufgefüllt, sondern ab 2
  der erste freie Name genommen; Obergrenze 999, danach Fehler).
- Mit Cache-Eintrag: Verzeichnis wird in das Ziel **kopiert** (nicht verschoben — der Cache
  bleibt für die nächste Analyse), dann `git checkout` des Default-Branches. Liegt Cache und Ziel
  auf demselben Volume, `cp -c` (APFS-Clone, praktisch kostenlos).
- Ohne Cache-Eintrag: normaler `git clone -- <url> <ziel>` mit Fortschritt.
- Danach Finder-Reveal. Ein halb angelegter Ordner bei Fehler wird wieder entfernt.
- Auslösung: Knopf „Klonen" in der Vorschau + ⌘K (Akkord, weil das Panel den Fokus nicht nimmt
  — siehe die ⌘E/⌘P-Lehre).
- IPC `repo_clone(url) -> PathBuf`, async + `spawn_blocking`.

## 4. Statistik-Erweiterung (`repo_stats.rs`)

Git-Log einmal parsen zu `Vec<Commit>` (Zeit, Autor, Dateien mit +/−), dann
`aggregate(commits, since) -> RepoStats` je Zeitraum. `repo_analyze` liefert
`RepoAnalysis { ranges: { d30, d90, d180, y1, all } }`. Die Zeiträume zählen ab dem
**letzten Commit** (nicht ab heute), damit ein ruhendes Repo nicht leer aussieht; die UI sagt das.

Neue Felder in `RepoStats`, alle aus reinen, getesteten Funktionen:

- `heatmap: [[u64; 24]; 7]` — Wochentag × autor-lokale Stunde.
- `calendar: Vec<DayCount>` — Tages-Commits der letzten 365 Tage des Zeitraums (nur `y1`/`all`).
- `hotspots: Vec<Hotspot { path, changes, authors }>` — viele Änderungen, höchstens 2 Autoren;
  Top 10.
- `bus_factor: u32` — kleinste Autorenzahl, die zusammen ≥ 50 % der Commits hält.
- `dir_bus_factor: Vec<DirStat { dir, commits, authors, bus_factor }>` — je Verzeichnis der
  obersten Ebene, Top 10.
- `co_change: Vec<Pair { a, b, count }>` — Paare, die oft gemeinsam geändert werden; Commits mit
  mehr als 30 Dateien zählen nicht mit (sonst quadratischer Aufwand); Top 10.

## 5. Private Repos

- Token-Quelle: `gh auth token` (falls `gh` installiert und eingeloggt), sonst optionales Token
  aus dem Schlüsselbund (Settings „Repositories“, Service `io.celox.inspector-rust`,
  Nutzer `github-token-v1`). Nie in DB, nie im Log.
- Übergabe nur an `github.com`, als Umgebungsvariablen
  `GIT_CONFIG_COUNT=1`, `GIT_CONFIG_KEY_0=http.https://github.com/.extraheader`,
  `GIT_CONFIG_VALUE_0=AUTHORIZATION: basic <base64(x-access-token:<token>)>` —
  **nie** in der Kommandozeile (sonst in `ps` sichtbar).
- Immer `GIT_TERMINAL_PROMPT=0`, damit Git nie auf eine Passworteingabe wartet.
- Scheitert die Anmeldung („Repository not found" / 401/403), zeigt die Vorschau einen
  Hinweis (`gh auth login` oder Token in Settings) statt eines rohen Git-Fehlers.

## 6. Oberfläche und Export

- `RepoPanel`: Zeitraum-Chips oben (30 T · 90 T · 180 T · 1 J · Gesamt), neue Karten für
  Heatmap, Kalender, Hotspots, Bus-Faktor (gesamt + Verzeichnisse), Co-Change; Knopf „Klonen"
  mit Fortschrittsbalken. Zeitraum-Wechsel rechnet nichts neu, er wählt nur den Datensatz.
- Export HTML/PDF über denselben Renderer (`build_html` + `report_style`), Grafiken inline-SVG,
  gewählter Zeitraum im Kopf. IPC `repo_export(target, format, range)`.

## 7. Fehlerfälle

- Kein Netz / Host nicht erreichbar → klarer Hinweis, Cache-Eintrag bleibt unverändert.
- Repo leer (keine Commits) → leere Statistik mit Hinweis, kein Absturz.
- Zeitraum ohne Commits → Karte „keine Commits in diesem Zeitraum".
- Klonziel nicht beschreibbar → Fehler mit Pfad, kein halber Ordner.

## 8. Tests

- Rust: `parse_repo_url`-Tabelle, `free_dir_name`, `cache_evictions`, `parse_git_progress`,
  Aggregation je Zeitraum, Heatmap, Bus-Faktor, Hotspots, Co-Change (inkl. 30-Dateien-Grenze),
  Auth-Umgebung (Token nie im argv).
- Frontend: `parseRepoUrl` (gleiche Tabelle), Rangfolge (`repo-url` über `appEntry`),
  RepoPanel-Zeitraumwechsel + Klon-Knopf.
- Jeder neue Test einmal gegen eine absichtlich kaputte Version (Mutationsprobe).

## 9. Doku

CommandDoc `repo`, `features.txt`, `CHANGELOG.md`, `CLAUDE.md` (Abschnitt repo/export),
`docs/repo.md`, README-Matrix über `gen-docs`.
