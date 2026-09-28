# `convert` / `cv` — Einheiten & Währungen

`cv 165 mph` rechnet beim Tippen um und zeigt in der Vorschau **alle** Einheiten
derselben Kategorie; die eingegebene Einheit ist hervorgehoben.

## Eingabe

| Form | Bedeutung |
|---|---|
| `cv` | Kategorien-Browser mit eigener Eingabe (Wert, Einheit, Ziel) |
| `cv 165 mph` / `cv 165mph` | alle Geschwindigkeiten |
| `cv 165 mph in kmh` | Ziel hervorgehoben, **Enter fügt die Zahl ein** |
| `… to` / `nach` / `->` / `→` / `=` | gleichbedeutend mit `in` |
| `cv mph` | Wert 1 |
| `cv 2*60 km` | Rechenausdruck als Wert |
| `cv 20€` · `cv $5` · `cv 1.234,5 eur` | Währungssymbole, beide Dezimal-Konventionen |

`in` ist erst dann das Ziel-Schlüsselwort, wenn davor ein vollständiger Wert mit
Einheit steht: `cv 5 in` sind fünf Zoll, `cv 5 in in cm` rechnet sie um.

**Autocomplete.** Solange die Einheit unfertig ist (`cv 165 mp`), stehen links
Vorschläge mit Live-Beispiel („Geschwindigkeit · 265,5418 km/h"); Tab/→ oder
Enter übernimmt. Eine kurze, schon gültige Einheit (`cv 165 m` = Meter) bietet
zusätzlich die längeren Einheiten an, die mit ihr beginnen (mi, mm, mph …). Nach
`in` werden nur Einheiten derselben Kategorie vorgeschlagen.

**Vorschau.** Enter gibt ihr die Tastatur: ←/→ Kategorie, ↑/↓ Zeile, Enter fügt
die gewählte Zahl ein (maschinenlesbar, z. B. `265.54176`), Klick auf eine Zeile
kopiert sie. Chips, Wertfeld und Auswahllisten schreiben in die Suchleiste zurück —
die Suchleiste ist immer der vollständige Zustand.

## Kategorien

Länge · Fläche · Volumen (US und UK getrennt: `gal` vs `ukgal`) · Masse ·
Temperatur (affin) · Geschwindigkeit · Zeit · Daten (SI `GB` = 10⁹ **und** binär
`GiB` = 2³⁰) · Energie · Leistung (`PS`, `hp`) · Druck · Verbrauch (L/100 km ↔ km/L ↔
mpg, reziprok) · Winkel · Währung.

Die Registry ist `core/frontend/src/lib/units.ts`; ein Test verbietet Schreibweisen,
die auf zwei Einheiten zeigen. Der ältere Inline-Konverter (`5 km in mi` ohne
Befehl) greift für Einheiten, die er selbst nicht kennt, auf dieselbe Registry zurück.

## Währungen

* **EZB-Referenzkurse** über `api.frankfurter.dev` (ca. 30 Währungen, werktäglich
  gegen 16 Uhr MEZ) — keine Kauf-/Verkaufskurse einer Bank.
* **Bitcoin, Ether** über die öffentliche CoinGecko-API.
* Cache in der Einstellungs-Tabelle (`fx.cache`): EZB 6 h, Krypto 10 min. Schlägt
  eine Aktualisierung fehl, bleibt der letzte Stand sichtbar und ist als *veraltet*
  markiert; ohne jeden Stand nennt die Vorschau den Fehler.
* Nur die Kurs-URLs verlassen den Rechner, nie ein Betrag.
* Eine Währung ohne Kurs wird ausgeblendet, nie mit 0 gerechnet.

Keine Finanzberatung.
