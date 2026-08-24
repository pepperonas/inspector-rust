# `repo` / `export` — Git-Repo-Statistik (repo2viz-Stil)

Wertet die git-Historie eines Repositories aus und zeigt sie in der Preview —
orientiert am **repo2viz**-Projekt (`~/claude/repo2viz`), das ein Repo
read-only klont, `git log --numstat` parst und eine interaktive Auswertung
rendert. Hier passiert dasselbe im Popup, plus ein HTML-Export.

## Aufruf

| Eingabe | Wirkung |
|---|---|
| `repo https://github.com/user/projekt` | klont read-only (bare) und analysiert |
| `repo /pfad/zum/repo` | analysiert einen lokalen Ordner in-place |
| `repo` (ohne Argument) | analysiert den **im Finder ausgewählten** Ordner, wenn er ein `.git` enthält |
| `export [url\|pfad]` | analysiert **und** speichert die HTML-Auswertung |

## Was gezeigt wird

- **KPI-Kacheln:** Commits, Mitwirkende, aktive Tage, längste Commit-Serie,
  Zeilen ein/aus.
- **Aktivitäts-Timeline:** Commits pro Monat als Sparkline.
- **Wochentag & Uhrzeit:** wann committet wird (Balken), inkl. Spitzenzeit.
- **Commit-Kategorien:** feat/fix/refactor/… aus Conventional-Commit-Präfixen.
- **Hotspots:** aktivste Dateien (nach Änderungshäufigkeit + Churn),
  Dateitypen, Top-Mitwirkende.

## HTML-Export

Der ⬇-Button (oder Taste **E** in der Ansicht, oder der `export`-Command)
schreibt eine **einzelne, eigenständige HTML-Datei** nach `~/Downloads` —
benannt `<owner>-<repo>-activity.html` (bei lokalen Repos der Ordnername).
Die Datei ist self-contained (Inline-CSS, CSS-Balkencharts, **keine** externen
Requests, kein Script), im dunklen MD3-Look — wie die repo2viz-Ausgabe.

## Genauigkeit & Grenzen

- **URL** → voller bare-Clone (Historie **inkl. Blobs**, damit `--numstat`
  den Churn exakt offline berechnet, kein Working-Tree). Sehr große Repos
  dauern entsprechend; danach wird der Temp-Klon gelöscht.
- **Lokal** → keine Kopie, direkt `git log` im Ordner (sofort).
- Merges werden ausgelassen (`--no-merges`); Zeiten sind autor-lokal (aus
  `%aI`). Braucht `git` im PATH.
- Der Parser läuft über Kontroll-Zeichen-getrennte Records (RS `\x1e`, US
  `\x1f`), sodass Commit-Texte die Feldtrennung nie zerstören; alle
  eingebetteten Namen werden im HTML escaped.

## Abgrenzung zu repo2viz

Die volle repo2viz-App kann mehr (Azure-DevOps-Work-Items, DORA-Metriken,
PO-Dashboard, Chart.js-Interaktivität). Die Preview ist der schnelle Überblick
+ ein sauberer HTML-Export; für die Tiefe bleibt repo2viz.
