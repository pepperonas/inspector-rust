# `repo` / `export` — Git-Repo-Statistik und Klonen (repo2viz-Stil)

Wertet die Git-Historie eines Repositories aus und zeigt sie in der Preview —
orientiert am **repo2viz**-Projekt (`~/claude/repo2viz`), das ein Repo
read-only klont, `git log --numstat` parst und eine interaktive Auswertung
rendert. Hier passiert dasselbe im Popup, plus Export als HTML/PDF und seit
v0.183.0 Klonen mit einem Tastendruck.

## Aufruf

| Eingabe | Wirkung |
|---|---|
| `https://github.com/user/projekt` (ohne Stichwort) | eigene Zeile **„Repo analysieren · user/projekt"**, Enter analysiert |
| `repo https://github.com/user/projekt` | wie oben, explizit |
| `repo /pfad/zum/repo` | analysiert einen lokalen Ordner in-place |
| `repo` (ohne Argument) | analysiert den **im Finder ausgewählten** Ordner, wenn er ein `.git` enthält |
| `export [url\|pfad]` | analysiert **und** speichert die HTML-Auswertung |

### Welche URLs erkannt werden

`https://github.com/o/r`, mit `.git`, mit `/tree/…` oder `/blob/…` (auf das
Repo gekürzt), ohne Schema (`github.com/o/r`) und `git@github.com:o/r.git`.
**Keine** Repo-Zeile gibt es für Issues, Pull Requests, Actions, Releases,
Wikis, Gists, Profilseiten und GitHub-Systempfade — sonst würde ein
eingefügter Issue-Link einen Klon auslösen. Owner und Repo werden gegen
GitHubs Zeichenregeln geprüft, damit aus einer URL nie eine Git-Option oder
ein Pfad-Ausbruch werden kann. Die Erkennung existiert zweimal (TypeScript
für die Zeile, Rust für das Backend) und beide prüfen **dieselbe**
Beispieldatei (`core/frontend/src/lib/repo-url-cases.json`).

Enthält ein **Clip** eine Repo-URL, zeigt seine Vorschau zusätzlich den Knopf
„Repo analysieren".

## Zeiträume

Chips **30 T · 90 T · 180 T · 1 J · Gesamt** schalten sofort um — die
Auswertung wird einmal gerechnet, jeder Zeitraum liegt schon vor. Gezählt
wird **ab dem letzten Commit**, nicht ab heute: bei einem Repo, an dem seit
Monaten niemand arbeitet, wären „30 Tage" sonst leer. Ein Zeitraum ohne
Commits zeigt „Keine Commits in diesem Zeitraum".

## Was gezeigt wird

- **KPI-Kacheln:** Commits, Mitwirkende, aktive Tage, längste Commit-Serie,
  Zeilen ein/aus, Bus-Faktor.
- **Aktivitäts-Timeline:** Commits pro Monat als Sparkline.
- **Wochentag & Uhrzeit:** wann committet wird, inkl. Spitzenzeit.
- **Heatmap Wochentag × Stunde.**
- **Beitragskalender** im GitHub-Stil (bei „1 J" und „Gesamt").
- **Commit-Kategorien:** feat/fix/refactor/… aus Conventional-Commit-Präfixen.
- **Aktivste Dateien, Dateitypen, Mitwirkende.**
- **Hotspots:** Dateien mit mindestens 3 Änderungen und höchstens 2 Autoren —
  hier hängt Wissen an wenigen Personen.
- **Bus-Faktor:** wie viele Autoren zusammen mindestens 50 % der Commits
  machen, gesamt und je Verzeichnis der obersten Ebene.
- **Co-Change:** Dateipaare, die oft im selben Commit geändert werden.
  Commits mit mehr als 30 Dateien zählen dafür nicht (Formatierungs- oder
  Umbenennungs-Commits würden alles mit allem koppeln, und der Aufwand wüchse
  quadratisch).

## Export

**⌘E** (HTML) und **⌘P** (PDF), oder die Knöpfe im Panel, schreiben den
**gewählten Zeitraum** nach `~/Downloads` — benannt
`<owner>-<repo>-activity.html`, bei einem Zeitraum mit Suffix
(`…-activity-d90.pdf`). Der Export nimmt die schon berechnete Statistik aus
dem Panel und klont **nicht** erneut. Die Datei ist self-contained (Inline-CSS,
Grafiken als Inline-SVG, keine externen Requests, kein Script) und nutzt das
gemeinsame Report-Design.

## Klonen

**⌘K** oder der Knopf „Klonen" legt das Repo im **Klon-Ordner** ab
(Settings → Repositories; Vorgabe `~/claude`, falls vorhanden, sonst
`~/Downloads`) und zeigt es im Finder. Ist der Name belegt, entsteht
`name (2)`, `name (3)` … bis `(999)` — gezählt ab 2, der erste freie Name
gewinnt; **nichts wird überschrieben**. Schlägt das Klonen fehl, bleibt kein
halber Ordner zurück.

## Cache

GitHub-Repos werden beim ersten Analysieren mit `--no-checkout` in
`~/Library/Caches/InspectorRust/repos/<owner>/<repo>` geklont; jede weitere
Analyse braucht nur ein `git fetch`. Gelesen wird dabei `origin/HEAD`, weil
der lokale Branch eines No-Checkout-Klons nach einem Fetch veraltet ist.
„Klonen" kopiert diese Kopie in den Zielordner (auf demselben Laufwerk per
APFS-Clone praktisch kostenlos) und checkt die Dateien aus — es wird nichts
ein zweites Mal übertragen. Der Cache hält höchstens **5 Repos oder 2 GB**; der
am längsten nicht benutzte Eintrag fliegt zuerst.

Nicht-GitHub-URLs laufen wie bisher über einen temporären bare-Klon, der nach
der Analyse gelöscht wird.

## Private Repos

Das Token kommt aus `gh auth token` (wenn die GitHub-CLI installiert und
eingeloggt ist), sonst aus dem Schlüsselbund (Settings → Repositories). Es
geht **nur** an `https://github.com/`, und zwar als Umgebungsvariable
(`GIT_CONFIG_*` mit `http.extraheader`) — **nie** auf die Kommandozeile, wo es
in der Prozessliste sichtbar wäre — und wird aus Fehlermeldungen entfernt.
`GIT_TERMINAL_PROMPT=0` verhindert, dass Git hängend auf eine Passworteingabe
wartet. Ohne Zugriff zeigt das Panel „Kein Zugriff auf das Repository" mit
dem Hinweis auf `gh auth login`.

## Genauigkeit & Grenzen

- **Lokal** → keine Kopie, direkt `git log` im Ordner (sofort).
- Merges werden ausgelassen (`--no-merges`); Zeiten sind autor-lokal (aus
  `%aI`). Braucht `git` im PATH.
- Der Parser läuft über Kontroll-Zeichen-getrennte Records (RS `\x1e`, US
  `\x1f`), sodass Commit-Texte die Feldtrennung nie zerstören; alle
  eingebetteten Namen werden im HTML escaped.
- Ein leeres Repo (keine Commits) wird als „keine Commits" angezeigt, nicht
  als Fehler.

## Abgrenzung zu repo2viz

Die volle repo2viz-App kann mehr (Azure-DevOps-Work-Items, DORA-Metriken,
PO-Dashboard, Contributor-Filter, Chart.js-Interaktivität). Die Preview ist
der schnelle Überblick mit Zeiträumen, Export und Klonen; für die Tiefe
bleibt repo2viz.
