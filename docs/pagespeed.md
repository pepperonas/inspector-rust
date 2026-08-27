# `pagespeed` — Google PageSpeed Insights

Misst eine Seite mit Googles PageSpeed Insights (Lighthouse) und zeigt die vier
Kategorien plus die Kernmetriken — **Desktop und Mobil zusammen**.

```
pagespeed <url>
```

Eine bloße Domain reicht: `celox.io` wird zu `https://celox.io`.

---

## Warum beide Strategien immer zusammen

Eine Seite ist regelmäßig auf dem Desktop gut und mobil schlecht. Wer nur eine
Hälfte sieht, zieht den falschen Schluss — deshalb holt ein Lauf **beide**
(parallel, sonst wäre es eine Minute Warten) und zeigt sie nebeneinander, in
einem festen Vierer-Raster, damit die Werte Kategorie für Kategorie auf einer
Höhe liegen. Der Export legt ebenfalls beide in **ein** Dokument.

---

## Bedienung

| Taste | Wirkung |
|---|---|
| Enter | Messung starten (zwei kalte Lighthouse-Läufe, je 10–40 s) |
| `R` | neu messen |
| `↑` `↓` | scrollen |
| Esc | schließen |

Export als **HTML** oder **PDF** über die Knöpfe; die Datei landet in
`~/Downloads` und wird im Finder gezeigt. Gestaltung und Fallstricke:
[`docs/reports.md`](./reports.md).

---

## Der API-Key

**Einstellungen → PageSpeed.** Der Key bleibt auf dem Rechner; er verlässt ihn
nur als `key`-Parameter der Anfrage an Google.

Ohne eigenen Key antwortet die API zwar, aber alle Nutzer teilen sich ein
kleines Tageskontingent — das ist in der Praxis schnell leer (beim ersten
schlüssellosen Versuch während der Entwicklung war es das bereits).

### ⚠️ IP-Beschränkung

Ein Google-Key kann auf bestimmte IP-Adressen begrenzt sein. Ein so gebundener
Key gilt **nur von der freigegebenen Adresse aus** und meldet sonst
„The provided API key has an IP address restriction". Das ist eine Einstellung
in der Google-Cloud-Konsole, nichts, was das Programm umgehen kann — deshalb
bekommt genau dieser Fall eine eigene, handlungsfähige Meldung.

Beobachtet: ein Key war auf die IPv4 eines Servers freigegeben und
funktionierte dort mit `curl -4`, scheiterte aber über IPv6 **vom selben
Rechner** und von jedem anderen.

---

## Werte lesen

| Band | Bereich | Farbe |
|---|---|---|
| gut | 90–100 | grün |
| verbesserungswürdig | 50–89 | bernstein |
| schlecht | 0–49 | rot |

Das sind Lighthouses eigene Schwellen. Sie stehen **zweimal** im Code — in
`pagespeed::band` (Rust) und `lib/pagespeed.ts` (Oberfläche) — weil Panel und
exportiertes PDF sich nicht darüber uneinig sein dürfen, was „gut" heißt; je
ein Test pinnt beide Seiten.

Eine Kategorie ohne Wert bleibt **unbekannt** und wird als `–` gezeigt, nie als
0: eine 0 läse sich als katastrophal für etwas, das Lighthouse schlicht nicht
messen konnte.

---

## Grenzen

- Zwei kalte Lighthouse-Läufe dauern je 10–40 Sekunden. Sie laufen parallel,
  aber sofort ist das nicht.
- Google misst von **Googles** Infrastruktur aus — die Seite muss öffentlich
  erreichbar sein. `localhost` schlägt in aller Regel fehl.
- Performance-Werte **schwanken zwischen Läufen**. Ein Export bildet immer den
  Lauf ab, der gerade im Panel steht; zwei Messungen derselben Seite können
  sich deutlich unterscheiden.
- Fehler werden **benannt, nicht verschluckt**: erschöpftes Kontingent und
  IP-beschränkter Key bekommen je eine eigene Meldung, statt als „keine Daten"
  zu erscheinen.

---

## Verwandt

- [`docs/reports.md`](./reports.md) — Gestaltung der Exporte
- Im Programm: `pagespeed?`
