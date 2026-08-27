# Exportierte Reports — ein Design für alle

Vier Befehle erzeugen Dokumente zum Weitergeben: **`loc`** (HTML · PDF · PNG),
**`pagespeed`** (HTML · PDF), **`repo`** (HTML) und die **Zeiterfassung**
(HTML ×2 + CSV). Sie teilen sich seit v0.143.0 ein einziges Stylesheet und ein
einziges Dokument-Gerüst: [`core/rust-lib/src/report_style.rs`](../core/rust-lib/src/report_style.rs).

Vorher trugen fünf Dokumente **vier handgeschriebene Stylesheets**, zwei davon
dunkel. Das ist die Art Drift, die niemand bemerkt, bis zwei Reports
nebeneinander liegen.

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
```

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
| `repo` | ✅ | — | — | Balkenreihen für Wochentag/Stunde/Monat |
| Zeiterfassung | ✅ | — | — | zwei Dokumente (Projekt-Report, Timesheet) + CSV |

**PNG ist bisher nur macOS** — der Schnappschuss hängt an WKWebView; anderswo
meldet der Befehl das ehrlich, statt eine leere Datei zu schreiben.

---

## Verwandt

- [`docs/disk.md`](./disk.md) · [`docs/repo.md`](./repo.md) · [`docs/timesheet.md`](./timesheet.md)
- Befehls-Doku im Programm: `loc?`, `pagespeed?`, `repo?`
