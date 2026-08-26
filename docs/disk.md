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
| 🗑 (Segment / Datei) | in den **Papierkorb** verschieben (mit Bestätigung), danach Neu-Scan |
| `R` / ⟳ | den **aktuell gezeigten** Ordner neu scannen |

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

## Nicht enthalten

Der volle DaisyDisk-Funktionsumfang (mehrere Datenträger nebeneinander,
Drag-in-Collector-Sammelmappe, Vorschau) bleibt der App vorbehalten — die
Preview ist der schnelle „wo ist mein Platz hin"-Blick.
