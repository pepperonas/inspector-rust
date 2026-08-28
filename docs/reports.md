# Exportierte Reports — ein Design für alle

Vier Befehle erzeugen Dokumente zum Weitergeben: **`loc`** (HTML · PDF · PNG),
**`pagespeed`** (HTML · PDF), **`repo`** (HTML) und die **Zeiterfassung**
(HTML ×2 + CSV). Sie teilen sich seit v0.143.0 ein einziges Stylesheet und ein
einziges Dokument-Gerüst: [`core/rust-lib/src/report_style.rs`](../core/rust-lib/src/report_style.rs).

Vorher trugen fünf Dokumente **vier handgeschriebene Stylesheets**, zwei davon
dunkel. Das ist die Art Drift, die niemand bemerkt, bis zwei Reports
nebeneinander liegen.

⚠️ **Und genau so ist es dann auch gekommen.** Die beiden Timesheet-Dokumente
wurden am selben Tag zwar hell und druckfertig gemacht — aber als **Kopie** der
Werte, nicht über das gemeinsame Modul; sie behielten ihre gerahmten Karten,
ihre eigene Pastellpalette und englische Beschriftung. Die Doku behauptete
trotzdem „ein Design für alle". Aufgefallen ist das erst, als die Dokumente
zum ersten Mal **gerendert und angesehen** wurden (v0.145.0). Ein Test hatte
die Umstellung sogar gepinnt — er prüfte die *Schreibweise* der kopierten
CSS-Regeln und war deshalb grün, während die Kopie danebenstand.

---

## Die kodierten Regeln

| Regel | Warum |
|---|---|
| **Hell, druckorientiert** | Ein Report ist etwas, das man druckt, anhängt oder einem Kunden gibt. Ein dunkles PDF verbrennt Toner und sieht auf Papier kaputt aus. |
| **A4, nicht Letter** | Die Leser sind hier. Letter ließ auf jeder gedruckten Seite einen Streifen stehen. |
| **Farbe nur, wo sie Daten trägt** | Kein dekorativer Akzent. Die Lighthouse-Bänder und die Serienfarben bedeuten etwas, der Rest ist Tinte, gedämpfte Tinte, Haarlinie. Das hält eine dichte Seite ruhig. |
| **Zahlen sind eine Spalte** | `tabular-nums` überall, rechtsbündig — Ziffer unter Ziffer. Der größte Lesbarkeitsgewinn in einer Datenauswertung. |
| **Data-Ink statt Rahmen** | Haarlinien und Weißraum statt Kästen; keine senkrechten Linien, keine Zebrastreifen. |
| **Anteil in der Zeile** | Der Balken steht **beim Namen**, nicht in einer eigenen Spalte, zu der das Auge wandern muss. |
| **Selbsttragend** | Eingebettetes CSS, eingebettetes SVG, kein Skript, keine Netz-Anfrage. Ein Report muss offline, in einem kopflosen WebView und in drei Jahren gleich aussehen. |

---

## Bausteine

```rust
report_style::css()                       // das Stylesheet
report_style::shell(kicker, title, subject, body, foot)
report_style::stats(&[Stat { label, value, unit }])   // Kennzahlen-Leiste
report_style::share_bar(&[(label, anteil, farbe)])    // Balken + Legende
report_style::name_cell(farbe, name, anteil)          // Zeilen-Label mit Spur
report_style::series_color(name)                      // Serienfarbe
report_style::pct(anteil)                             // 0.118 -> "11,8 %"
```

Klassen: `.rp-text` (linksbündige Textspalte), `.rp-num` (Zahl), `.rp-dim`
(gedämpft), `.rp-lede` (neutraler Erklärsatz), `.rp-note` (**bernsteinfarbener
Warnkasten** — nur für Vorbehalte), `.rp-empty` (nichts vorhanden).

Ein Report, der eigene Regeln braucht, hängt sie an: `doc.replace("</style>",
&format!("{EIGENES_CSS}\n</style>"))`. So bleibt die gemeinsame Basis
unangetastet.

---

## Fallstricke, alle real eingetreten

### `print-color-adjust: exact` ist tragend

Ohne diese Regel **verwirft WebKit beim PDF-Rendern jeden Hintergrund**.
Verloren gehen dann Anteilsbalken, Band-Chips und Ringe — also genau die
Teile, die die Aussage tragen. Der Report sieht nicht kaputt aus, er sieht
*leer* aus, was schlimmer ist. Ein Test pinnt die Regel.

### Die Seitengröße kommt aus den View-Bounds, nicht aus `@page`

`createPDFWithConfiguration: nil` nimmt das Seitenrechteck aus den **Bounds
der WebView**. Der Rahmen in [`md_to_pdf.rs`](../core/rust-lib/src/md_to_pdf.rs)
*ist* also das Papier: 794 × 1123 = A4 bei 96 dpi. `@page { size: A4 }` allein
hätte nichts geändert.

### Serienfarben brauchen eine Palette, keinen Hue-Hash

Ein Hash direkt auf 0–360° ist beständig, aber trennt nicht: zwei Sprachen im
selben Report kamen als **fast identische Türkistöne** heraus. Jetzt bildet der
Name deterministisch in eine kuratierte Zwölfer-Palette ab — Beständigkeit
*und* Unterscheidbarkeit. Ein Test verbietet Dubletten und zu helle Einträge.

### Nur `:first-child` ist linksbündig

Das genügt, solange ein Report **eine** Textspalte hat — `loc` und `repo` haben
genau das, und die Zahlen sind die letzten Spalten. Die Zeiterfassung hat
mehrere (Projekt, App, Host, Tätigkeit); ohne `.rp-text` rutschten sie alle
nach rechts und sahen wie Zahlen aus.

### Jedes Spaltenpaar braucht eine Rinne

`th + th, td + td { padding-left: 18px }`. Ohne sie stößt eine rechtsbündige
Zahlenspalte direkt an die nächste linksbündige Textspalte — gemessen:
`2.5010:00:00–12:30:00`. Bei `loc`/`repo` fällt das nie auf, weil dort die
Zahlen am Ende stehen. **Das sieht man nur im gerenderten Bild**, kein Test
hätte es gefunden.

### `.rp-note` ist ein Warnkasten, kein Fließtext

Bernstein auf Creme mit Rahmen — für den `inaccurate`-Vorbehalt gebaut. Ein
neutraler Erklärsatz darin liest sich als Warnung; dafür gibt es `.rp-lede`.

### Zahlen getrennt formatieren

`format!(…).replace('.', ",")` über die fertige Legendenzeile traf auch das
Label: aus `Node.js` wurde **`Node,js`**. Nur die Prozentzahl wird umgestellt.

### Tabellenköpfe über Seitenumbrüche

`thead { display: table-header-group }` — eine Tabelle, die auf Seite 2 läuft,
darf ihren Kopf nicht verlieren. Dazu `break-inside: avoid` auf Abschnitten,
Kennzahlen-Leisten und Zeilen.

### PNG: die Höhe darf nicht `scrollHeight` sein

`takeSnapshot` fotografiert die **Bounds**. Bei Inhalt, der kürzer als der
Rahmen ist, klemmt `scrollHeight` auf die Viewport-Höhe (gemessen: 1200 für ein
~400-pt-Dokument) — und bei längerem Inhalt hätte dieselbe naive Messung den
Report **abgeschnitten**. Gemessen wird die Unterkante des Fußtexts.

---

## Formate je Befehl

| Befehl | HTML | PDF | PNG | Besonderheit |
|---|:--:|:--:|:--:|---|
| `loc` | ✅ | ✅ | ✅ | Sprachen-Tabelle mit Anteils-Spur je Zeile |
| `pagespeed` | ✅ | ✅ | — | Desktop **und** Mobil nebeneinander in einem Dokument |
| `repo` | ✅ | ✅ | — | Balkenreihen für Wochentag/Stunde/Monat |
| Zeiterfassung | ✅ | ✅ | — | zwei Dokumente (Projekt-Report, Timesheet) + CSV |

**PDF überall, PNG nur wo das Dokument auf einen Blick passt.** Ein `loc`-Report
ist eine Karte, die man in einen Chat zieht; ein Timesheet über eine Woche ist
mehrseitig — davon ein Bild zu machen hieße, es unlesbar zu falten.

Alle vier gehen durch **einen** Schreiber (`commands::write_report`): HTML/CSV
direkt auf die Platte, PDF/PNG durch WebKit. ⚠️ **Jeder Aufrufer muss `async`
sein** — WebKit besteht auf dem Hauptthread, und ein synchroner
`#[tauri::command]` läuft selbst dort und wartete auf sich selbst.

**PNG ist bisher nur macOS** — der Schnappschuss hängt an WKWebView; anderswo
meldet der Befehl das ehrlich, statt eine leere Datei zu schreiben.

---

## Verwandt

- [`docs/disk.md`](./disk.md) · [`docs/repo.md`](./repo.md) · [`docs/timesheet.md`](./timesheet.md)
- Befehls-Doku im Programm: `loc?`, `pagespeed?`, `repo?`
