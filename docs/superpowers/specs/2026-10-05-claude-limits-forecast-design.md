# `limits` — Hochrechnung, Tempo und Fenster-Grafik — Design

Stand: 2026-10-05 · Status: Design aus der Token-Tracker-Session übernommen,
vom Nutzer für den Token Tracker freigegeben; **für Inspector Rust noch zur
Durchsicht**. Umsetzung in eigener Session: dieses Spec durchsehen, dann mit
`superpowers:writing-plans` den Implementierungsplan unter
`docs/superpowers/plans/` erstellen.

Schwester-Spec (Rechenmodell + API-Vertrag, maßgeblich):
`/Users/martin/claude/token-tracker/docs/superpowers/specs/2026-10-05-usage-forecast-design.md`

## Ziel

Das `limits`/`quota`-Panel (`ClaudeLimitsPanel.tsx`) beantwortet je Limit:

1. **Liege ich im Plan?** — Ist gegen die gleichmäßige Linie über das Fenster.
2. **Wann ist es aufgebraucht?** — 100 % vor dem Reset, und wann (mit Spanne).

Sichtbar im Balken jeder Zeile und als aufklappbare Fenster-Grafik bei den
Wochenlimits — dieselbe Darstellung wie im Token Tracker.

## Architekturentscheidung: Prognose vom Token Tracker übernehmen

Inspector Rust holt Token-Daten schon heute **über die HTTP-API des Token
Trackers** (`token_usage.rs`, `127.0.0.1:5010`) statt `~/.claude/projects`
neu zu parsen oder `tracker.db` zu öffnen — Begründung dort: Claude löscht alte
JSONL, die DB ist WAL-live, Preis-/Aggregationslogik lebt im Tracker. Für die
Prognose gilt dasselbe, noch stärker: Die Kalibrierung braucht die komplette
Kostenhistorie je Modell, das Wochenmuster 4 Wochen davon.

Deshalb:

- **Primärquelle:** `GET http://127.0.0.1:5010/api/usage-limits` → je Limit
  das Feld `forecast` (Vertrag `version: 1`, siehe Schwester-Spec, Abschnitt
  „API-Vertrag"). Inspector Rust **rechnet nicht nach**, es zeigt an.
- **Rückfall ohne Tracker** (Port zu, `ERR_UNREACHABLE`, oder Tracker-Version
  ohne `forecast`): lokal nur **Tempo + lineare Hochrechnung** aus dem
  eigenen Limits-Bericht (`percent`, `resets_at`, Fensterlänge) — reine
  Funktion in Rust, Basis `linear`, Güte `rough`, Hinweis „Für die genaue
  Prognose Token Tracker starten". Keine Vorwochen, kein Band außer der
  linearen ±40 %-Spanne, keine Grafik-Geister.
- Keine eigene Snapshot-Tabelle in `history.db` (YAGNI: der Tracker schreibt
  sie bereits; Inspector Rust pollt nur bei offenem Panel und hätte Lücken).

## Abgleich der Limits

Die IDs unterscheiden sich (Tracker `weekly_all`, Inspector Rust
`weekly_all-1`). Zuordnung je Limit über **`kind` + `resets_at`** (auf die
Minute gerundet) + bei `weekly_scoped` das Modell (`scopeLabel` im Tracker,
Modellname im Inspector-`name`). Kein Treffer → Rückfall linear für dieses
Limit. Codex: Tracker-Provider `codex`, Abgleich über `windowMinutes` +
`resets_at`. Antigravity: keine Prognose („keine Prozentwerte").

## Rust (`core/rust-lib/src/`)

- Neues Modul `limits_forecast.rs`:
  - Typen spiegeln den `forecast`-Vertrag (`serde`, alle Felder optional bzw.
    `#[serde(default)]` — fremde/neue Felder dürfen nichts brechen;
    unbekannte `version` > 1 → ignorieren und Rückfall).
  - `fetch_tracker_forecasts(base) -> Result<Vec<TrackerLimit>, String>`:
    dünne HTTP-Schicht im Stil von `token_usage::fetch` (Zeitlimit 4 s,
    `ERR_UNREACHABLE`).
  - Rein + getestet: `match_limit(&Limit, &[TrackerLimit]) -> Option<&Forecast>`,
    `linear_forecast(percent, resets_at, window_min, now) -> Forecast`
    (Formeln exakt wie im Schwester-Spec: `planPercent`, `deltaPoints`,
    Rate = `percent / vergangene Zeit`, `too_early` unter 10 % des Fensters,
    Status-Tabelle gleich).
- `claude_limits::status` hängt je Limit `forecast: Option<Forecast>` an
  (Claude und Codex), Tracker-Abfrage höchstens alle 60 s gecacht, nur wenn
  das Panel ohnehin fragt (kein neuer Hintergrund-Thread).
- Blockierende HTTP-Abfrage über `async` + `spawn_blocking` wie die übrigen
  IPC-Befehle.

## Frontend (`core/frontend/src/`)

- `lib/ipc.ts`: `ClaudeLimit.forecast?: LimitForecast | null` (Typ nach
  Vertrag).
- `lib/claude-limits.ts` — reine, mit Vitest getestete Helfer:
  `forecastText(f, now)` („7 Punkte Reserve · voraussichtlich 78 % · Reset
  Sa 01:00", „leer Do ~14:20 (Mi 22 – Fr 9 Uhr)", „leer in ~1:40 h",
  „grobe Schätzung"), `forecastTone(f)` (reserve → ok, ahead → warn,
  exhausts → crit), `barSegments(percent, f)` (Ist / Plan-Strich /
  Prognose-Schraffur / rote Kappe).
- `ClaudeLimitsPanel.tsx`:
  - **Ebene 1 — Balken:** Plan-Strich, schraffierte Prognose bis
    `atReset.median`, rote Kappe > 100 %, Statuszeile darunter.
  - **Ebene 2 — Fenster-Grafik** (nur Wochenlimits mit `series`, Klick auf die
    Zeile): **handgerolltes SVG** wie im `StatsPanel` (keine Chart-Bibliothek
    neu), `vectorEffect="non-scaling-stroke"`. Inhalt: Plan-Linie
    gestrichelt, Ist-Linie, gemessene Punkte, Median gestrichelt + Band, Marker
    am 100-%-Schnitt mit Uhrzeit, „jetzt"-Linie, bis zu 3 Vorwochen-Geister,
    Nachtstunden 22–7 Uhr schattiert, rote Zone über 100 %.
    Hover-Tooltip Ist / Plan / Prognose.
  - Im Rückfall (`basis: 'linear'`) nur Ebene 1 + Hinweis
    „Token Tracker starten für Verlauf und Prognose".
  - Aufgeklappte Grafiken in den App-Einstellungen gemerkt
    (`settings`-Tabelle, Schlüssel `climits.forecast_open`).
- `prefers-reduced-motion`: keine Animation der Grafik.

## Doku (Dokumentationsvertrag des Repos beachten)

`docs/claude-limits.md` um Abschnitt „Hochrechnung" (Quelle Tracker, Rückfall,
Abgleich, Grenzen) · `CommandDoc` des `limits`-Befehls · README-Featureliste
(DE/EN) · CHANGELOG · Abzeichen/Index laut CLAUDE.md-Checkliste.

## Tests

- Rust: Vertrags-Deserialisierung (vollständig, minimal, unbekannte Felder,
  `version: 2` → ignoriert), `match_limit` (Treffer über kind+resets_at,
  Scope-Modell, kein Treffer), `linear_forecast` (Tempo-Grenzen, `too_early`,
  Erschöpfung vor/nach Reset, Status-Tabelle), Tracker unerreichbar →
  Rückfall.
- Frontend (Vitest): `forecastText`/`forecastTone`/`barSegments`,
  Grafik-Pfadbau als reine Funktion (Skalierung, Schnittpunkt-Marker).
- Mutationsprobe je neuem Pin; Sichtprüfung im laufenden Panel mit und ohne
  Token Tracker.

## Abhängigkeit

Setzt Token Tracker ≥ 0.8.0 mit `forecast` in `/api/usage-limits` voraus
(fehlt es, greift automatisch der lineare Rückfall — Inspector Rust bleibt
mit älteren Trackern lauffähig).

## Nicht im Umfang

Eigene Kalibrierung/Snapshots in Inspector Rust · Antigravity-Prognose ·
Benachrichtigungen · Menüleisten-Anzeige der Prognose.
