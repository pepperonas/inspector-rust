# `disk` / `daisy` — Speicher-Analyse (DaisyDisk-Stil)

Nachbau der Kernfunktion der macOS-App **DaisyDisk** direkt in der IR-Preview:
ein **Sonnenkranz-Diagramm** (Sunburst) der Ordnergrößen. `disk` (oder
`daisy`) tippen, Enter.

## Was es zeigt

- **Konzentrische Ringe:** der innerste Ring sind die direkten Unterordner des
  gescannten Ordners, jeder weitere Ring eine Ebene tiefer. Der **Winkel** eines
  Segments ist proportional zum belegten Platz.
- **Mitte (Hub):** Name, Größe und Anteil des Elements, über dem die Maus
  schwebt — sonst des aktuellen Ordners.
- **Volume-Leiste:** freier Speicher des Datenträgers und wie viel davon der
  gescannte Ordner ausmacht (DaisyDisks „freier Platz").
- **Größte Dateien:** eine Liste der dicksten Einzeldateien — über den **ganzen**
  Scan berechnet, auch für Dateien, die im Diagramm unter „Sonstiges" fallen.

## Bedienung

| Aktion | Wirkung |
|---|---|
| Klick auf Segment | in den Ordner zoomen (Drill-down) |
| Klick in der **Pfadleiste** | direkt zu diesem Verzeichnis springen |
| `⌫` / `←` / ↰ | eine Ebene höher — **auch über den Startordner hinaus** |
| Esc | eine Ebene zurück; am Wurzelknoten schließt es das Panel |
| Hover | Details in der Mitte + Detailzeile |
| Leertaste / ＋ | die gewählte Zeile (Ordner **oder** Datei) in den **Sammler** nehmen — auch über Ordner hinweg |
| `⌘⌫` / Entf | den Sammler in den **Papierkorb** — zweistufig: einmal armiert (rot, mit Hinweis), nochmal bestätigt; Esc bricht ab. Bei leerem Sammler trifft es die gewählte Zeile |
| **Rechtsklick** auf ein Segment | Kontextmenü mit vollem Pfad → mit einem Klick sofort in den **Papierkorb** (s. u.) |
| ＋ am Segment / an der Datei | in den Sammler (nie direkt löschen) |
| `R` / ⟳ | den **aktuell gezeigten** Ordner neu scannen |

## Löschen — der Sammler

DaisyDisks Kern-Workflow **finden → sammeln → löschen**, in Tastaturform:

1. Mit `↑↓` eine Zeile wählen, **Leertaste** nimmt sie in den Sammler (＋ mit
   der Maus). Das geht mit Ordnern **und** Dateien, und über Ebenen hinweg —
   erst `target` an der Wurzel, dann tiefer `src/alt.log`, alles in einer
   Sammlung mit laufender Gesamtgröße.
2. **`⌘⌫`** (die Finder-Geste „In den Papierkorb") armiert: der Knopf wird rot
   und sagt, was passiert. Ein zweites `⌘⌫` (oder Klick) verschiebt alles.
   Esc, ein Wechsel der Auswahl oder vier Sekunden Untätigkeit entwaffnen.
3. Ist der Sammler leer, trifft `⌘⌫` die **gewählte Zeile** direkt — derselbe
   zweistufige Ablauf, nur ohne Sammelschritt.

Danach wird **nicht neu gescannt**: die Einträge verschwinden aus Diagramm
und Liste, die Ordnergrößen sinken um genau diese Bytes, und du stehst noch
im selben Ordner. Ein Voll-Scan von `~` dauert Sekunden und warf früher
zurück zur Wurzel — drei Dinge tief im Baum löschen hieß dreimal neu
hinnavigieren.

Zwei Ehrlichkeiten: Es gibt **nur den Papierkorb** (wiederherstellbar), kein
endgültiges Löschen — das gehört zu `clean` mit seiner Allowlist. Und der
**freie Platz des Volumes ändert sich erst, wenn du den Papierkorb leerst**;
die Volume-Leiste bleibt darum absichtlich stehen. Was nicht verschoben
werden konnte (Rechte, Pfad inzwischen weg), steht mit Grund unter dem
Sammler und bleibt darin, damit nichts stillschweigend verloren geht.

## Rechtsklick-Menü

Ein **Sekundärklick auf ein Ring-Segment** öffnet ein Kontextmenü, das den
getroffenen Ordner bzw. die Datei mit vollem Pfad nennt und ihn mit einem Klick
in den **Papierkorb** verschiebt — Finders Sekundärklick, für den Fall, dass man
den Sammler nicht braucht. Der Papierkorb ist wiederherstellbar, deshalb ist der
Rechtsklick plus Menüklick bereits die bewusste Aktion (keine zusätzliche
Rückfrage). Esc oder ein Klick daneben schließt das Menü; danach aktualisiert
sich die Ansicht wie beim Sammler (lokaler Prune, kein Neu-Scan). Das synthetische
„Sonstiges"-Segment hat keinen Pfad und öffnet darum kein Menü.

## Navigation

Die **Pfadleiste** über dem Diagramm nennt immer den absoluten Pfad dessen, was
gerade zu sehen ist. Sie ist vollständig anklickbar, und die beiden Fälle
verhalten sich bewusst unterschiedlich:

- Ein Segment **innerhalb** des gescannten Baums springt **sofort** — die Größen
  sind längst berechnet, ein erneuter Lauf wäre reine Verschwendung.
- Ein Segment **oberhalb** der Scan-Wurzel scannt dort neu. Genau dadurch kann
  man aus dem Startordner herauslaufen und den ganzen Datenträger durchsehen,
  ohne je wieder einen Pfad zu tippen.

Derselbe Unterschied gilt beim Hineinzoomen: Ordner **im** Baum öffnen sich
verzögerungsfrei, ein Ordner an der **Grenze des Walks** — dort ist nichts mehr
zu zeigen — wird frisch gescannt. Damit ist die Tiefe praktisch unbegrenzt.

## Ziele

- `disk` — der **im Finder ausgewählte Ordner**, sonst dein **Home-Verzeichnis**.
- `disk <pfad>` — ein konkreter Ordner, z. B. `disk ~/Downloads`.
- `disk /` — das **ganze Volume**; dann gehört der freie Speicher mit ins Bild.

## Genauigkeit & Grenzen

- **On-Disk-Größe** (belegte Blöcke × 512), nicht die scheinbare Größe — deckt
  sich mit der Volume-Anzeige. Symlinks werden **nicht** verfolgt, der Scan
  **bleibt auf einem Dateisystem** (keine Netz-/externen Mounts).
- Ein voller Home-/Volume-Scan läuft über 10⁵–10⁶ Dateien und dauert ein paar
  Sekunden; ein Live-Zähler zeigt den Fortschritt.
- Geschützte Systempfade unter `/` brauchen ggf. „Full Disk Access" in den
  Systemeinstellungen. Nicht lesbare Ordner werden übersprungen (nie fatal).
- Das Diagramm ist **begrenzt** (Top-Ordner je Ring, ~5 Ringe) — der Rest
  fällt in ein „Sonstiges"-Segment, damit es lesbar bleibt; die
  Größte-Dateien-Liste rechnet über alles.
- Das Diagramm **füllt die Preview-Breite und wird nie am Rand abgeschnitten**:
  die SVG-viewBox wird aus der Ring-Geometrie abgeleitet (`2×(hubR+Ringe·Ring)`
  plus Rand), umschließt damit den äußersten Ring immer und skaliert linear auf
  die verfügbare Breite herunter.

## Nicht enthalten

Der volle DaisyDisk-Funktionsumfang (mehrere Datenträger nebeneinander,
Vorschau, Drag-and-Drop in den Sammler — hier ist er Tastatur und ＋) bleibt
der App vorbehalten — die Preview ist der schnelle „wo ist mein Platz hin"-Blick,
der den gefundenen Platz auch gleich freiräumt.
