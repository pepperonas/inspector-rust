# `8bit` / `16bit` — Live-Retro-Overlay

`8bit` legt ein bildschirmfüllendes, click-through Overlay über den Bildschirm,
das den Bildschirm live abgreift und in Retro-Optik zeigt: Pixelung,
Farbreduktion, Dithering, auf Wunsch Scanlines und CRT-Krümmung. Gedacht als
Show und Spaß — der Rechner muss dabei aber jederzeit normal bedienbar bleiben.

## Bedienbar bleiben (seit 0.192.0)

Die erste Fassung (0.190.0) hat den ganzen Bildschirm samt Menüleiste, Dock und
aktivem Fenster durch eine verzögerte, verpixelte Kopie ersetzt. Text war
unlesbar, Klicks gingen daneben, und Menüs und Dialoge erschienen als Pixelbrei
einen Frame zu spät. Drei Regeln beheben das:

1. **System-UI bleibt echt.** Das Overlay liegt auf Fensterebene 1, also
   *unter* Menüleiste (24), Dock (20), aufgeklappten Menüs (101), schwebenden
   Panels (3), Dialogen und Mitteilungen. Diese Fenster (alles mit
   `windowLayer > 0`) werden zusätzlich aus dem Capture genommen, damit ihre
   Kopie nicht verzögert darunter durchscheint. Ein Hintergrund-Thread prüft
   alle 300 ms die Fensterliste und aktualisiert den Capture-Filter nur, wenn
   sich die Menge geändert hat (`updateContentFilter`, kein Neustart).
2. **Das aktive Fenster bleibt echt.** Standard ist „Aktives Fenster zeigt:
   Original" — dort ist das Overlay **durchsichtig**, man sieht das echte
   Fenster ohne jede Verzögerung. Ein dünner Rahmen in Palettenfarbe liegt
   *außen* um das Loch, damit der Fensterinhalt unberührt bleibt. Die frühere
   Darstellung „feine Pixel" gibt es weiter als Option. Ein App-Wechsel wird
   innerhalb von 0,1 s erkannt (vorher 0,5 s).
3. **Zurückweichen beim Arbeiten.** Beim Tippen, Scrollen oder Ziehen
   verschwindet das Overlay **sofort** und kommt erst nach 0,9 s Ruhe
   weich zurück. Gemessen wird nur echte Eingabe
   (`CGEventSourceSecondsSinceLastEventType` im HID-Zustand für keyDown,
   scrollWheel und die drei Drag-Ereignisse) — Mausbewegung allein zählt
   nicht, sonst wäre das Overlay nie zu sehen. Abschaltbar.

Dazu kommt Last: das aktive Fenster als Loch und die Lupe im Modus „Original"
brauchen keine feinen Bildpunkte mehr. Der Capture läuft deshalb fast immer
in **Zellauflösung** statt in voller Retina-Auflösung (vorher 2992×1934 bei
60 fps, sobald der Fokus-Modus an war).

**Grenze:** Ein Vollbild-Fenster als aktives Fenster ist ein Loch über den
ganzen Monitor — dort ist das Overlay dann praktisch aus. Für eine
Vollbild-Show schaltet ⌃⇧⌥9 die Ausnahme ab.

| Plattform | Stand |
|---|---|
| macOS 12.3+ | vollständig, im Betrieb geprüft |
| Windows 10 2004+ | vollständig implementiert, **nicht im Betrieb geprüft** (nur Cross-Compile) |
| Linux | nicht unterstützt — das Panel sagt es, statt einen Fehler zu zeigen |

## Zwei Modi

| | 8-Bit (Show) | 16-Bit (Alltag) |
|---|---|---|
| Farben | feste Palette: NES, C64, PICO-8, Game Boy, CGA, Graustufen | Farbtiefe pro Kanal: SNES (5 Bit), Mega Drive (3 Bit), Amiga OCS (4 Bit) |
| Pixelgröße | 3–12 pt | 1–4 pt |
| Gedacht für | Musik, Videos, Präsentationen, Spaß | bedienbarer Alltag, lesbarer Text im aktiven Fenster |

**Lesbarkeit hängt an der Pixelgröße, nicht an der Farbtiefe.** Deshalb gibt
es den Fokus-Modus und die Cursor-Lupe (siehe unten).

Jeder Modus hat seinen **eigenen Satz Einstellungen**. Ein Moduswechsel zeigt
deshalb die Standardwerte des anderen Modus, solange du dort nichts geändert
hast, und danach deine Werte.

Pixelgrößen sind **logische Punkte** und werden pro Monitor mit dessen
Skalierungsfaktor in physische Pixel umgerechnet (Retina 2×: 2 pt = 4 px). Eine
Einstellung sieht auf Retina- und Nicht-Retina-Monitoren gleich aus, auch
gemischt.

## Bedienung

| Eingabe | Wirkung |
|---|---|
| `8bit` | Panel mit Live-Vorschau im zuletzt genutzten Modus; Enter schaltet das Overlay an/aus |
| `16bit` | dasselbe, direkt im 16-Bit-Modus |
| `8bit on` / `8bit off` (auch `an`/`aus`) | starten / beenden |
| `8bit <palette>` (`gb`, `pico8`, `snes`, …) | mit dieser Palette starten, der Modus folgt aus der Palette |
| `8bit <preset>` (`alltag`, `show`, `retro-arbeit`, eigene) | Preset starten |
| `8bit focus` / `8bit lens` (auch `fokus`/`lupe`) | Ausnahme fürs aktive Fenster bzw. Lupe umschalten, auch im laufenden Betrieb |
| `8bit ?` | Inline-Hilfe |
| **⌃⇧⌥8** | Overlay an/aus — von überall, der Notausstieg (umbelegbar) |
| **⌃⇧⌥9** | Ausnahme fürs aktive Fenster an/aus (umbelegbar) |
| Tray → „Retro-Overlay beenden“ | zweiter Notausstieg, nur aktiv solange das Overlay läuft |
| `--retro` / `--retro-preset <name>` | dasselbe von der Kommandozeile (an die laufende Instanz) |

Im Panel: **↑↓** Zeile, **←→** Wert, **Enter** Overlay an/aus, **Esc** zurück.
Jede Änderung wirkt sofort auf die Vorschau und auf ein laufendes Overlay und
wird gespeichert.

## Einstellungen

| Gruppe | Einstellung | Bereich |
|---|---|---|
| Bild | Modus | 8-Bit / 16-Bit |
| | Palette / Farbtiefe | je Modus, siehe oben |
| | Pixelgröße | 8-Bit 3–12 pt, 16-Bit 1–4 pt, 0,5er-Schritte |
| | Dithering | aus, Bayer 2×2 / 4×4 / 8×8 |
| | Dither-Stärke | 0–100 % |
| Lesbarkeit | Aktives Fenster ausnehmen | an/aus (Standard an) |
| | Aktives Fenster zeigt | **Original** (Loch, Standard) / feine Pixel |
| | Pixel im Fokus | 1–4 pt, nur bei „feine Pixel"; **1 pt = nur Farbreduktion** |
| | Rahmen ums Fokusfenster | an/aus, Farbe aus der Palette (bei „Original" außen ums Loch) |
| | Cursor-Lupe | an/aus |
| | Lupen-Radius | 40–200 pt |
| | In der Lupe | Original (Loch) / Fokus-Pixelgröße |
| | Beim Tippen/Scrollen ausblenden | an/aus (Standard an), gilt für beide Modi |
| Röhre | Scanlines + Intensität | an/aus, 0–100 % |
| | CRT-Krümmung & Vignette + Stärke | an/aus, 0–100 % |
| | Deckkraft | 50–100 % |
| Stufe 2 | Retro-Fensterrahmen | Pixelrahmen + Titelleiste in Pixelschrift |
| | Sprite-Cursor | Pixel-Pfeil an der Mausposition |
| Ausgabe | Monitor | aktueller / alle |
| | FPS-Limit | 30 / 60 |

**Presets:** „Show“ (8-Bit, NES, 4 pt, Bayer 4×4 60 %, Scanlines), „Alltag“
(16-Bit, SNES, 2 pt, Bayer 4×4 25 %) und „Retro-Arbeit“ (8-Bit, PICO-8, 4 pt,
Lupe 80 pt) — alle mit echtem aktivem Fenster. Eigene Presets speichern die
aktuellen Werte des aktiven Modus. Ein Preset darf nicht wie eine Palette oder
ein Befehlswort heißen, damit `8bit <name>` eindeutig bleibt. Mitgelieferte
Presets lassen sich nicht löschen. **Zurücksetzen** setzt nur den aktiven Modus
auf seine Standardwerte.

**Umstellung gespeicherter Einstellungen:** Ein gespeicherter Modus aus 0.190/0.191
kennt das Feld `focus_native` noch nicht. Beim ersten Laden wird für ihn einmalig
das echte aktive Fenster eingeschaltet (`focus` + `focus_native`); danach gilt,
was man wählt.

Gespeichert wird im Settings-System: `retro.config` (beide Modi) und
`retro.presets` (nur eigene).

## Aktives Fenster und Lupe

- **Original (Standard):** Über dem aktiven Fenster gibt der Shader
  `float4(0)` zurück, das Overlay ist dort durchsichtig. Der Rahmen liegt außen.
- **Feine Pixel:** Das fokussierte Fenster wird mit eigener, feinerer
  Zellgröße gezeichnet. Sein Rechteck wird nach außen auf das
  Hintergrundraster ausgerichtet, damit beim Verschieben nichts flimmert.
  Bei Vollbild-Apps gilt der ganze Monitor als Fokus.
- **Quelle:** ereignisbasiert, ohne Abfrage im Bildtakt. macOS nutzt einen
  `AXObserver` auf die vorderste App (Fokuswechsel, Verschieben,
  Größenänderung), Windows WinEvent-Hooks.
- **Ohne Berechtigung:** Fehlt die Bedienungshilfen-Berechtigung (macOS),
  bleibt der Fokus unbekannt. Alles wird dann gleich grob gezeichnet, und das
  Panel weist darauf hin.
- **Cursor-Lupe:** Ein Kreis um die Maus zeigt das echte Bild (ein Loch,
  ohne Verzögerung) oder die Fokus-Pixelgröße. Seine Kante ist auf das Hintergrundraster gestuft
  (Pixel-Look). Er folgt der Maus ohne Verzögerung, bewegt sich die Maus
  nicht, wird nichts neu gezeichnet.

## Stufe 2

Retro-Fensterrahmen ziehen um jedes normale Fenster einen Pixelrahmen und eine
Titelleiste in einer 5×7-Pixelschrift, in der hellsten und dunkelsten Farbe der
Palette. Das fokussierte Fenster bekommt eine helle Leiste. Verdeckte Fenster
bekommen keinen Rahmen über das vordere. Fenstergrößen und Titel kommen aus
`CGWindowList` (macOS) bzw. `EnumWindows` (Windows), aktualisiert bei
Fensterereignissen und höchstens einmal pro Sekunde.

Der **Sprite-Cursor** erscheint **neben** dem echten Cursor. Den
System-Cursor kann ein click-through Overlay nicht ausblenden, er liegt
systembedingt über jedem Fenster. Text per OCR neu zu setzen ist nicht Teil
des Features.

## Funktionsweise

```
Bildschirm ──Capture (ohne Overlay-Fenster)──► Textur ──Shader──► Overlay-Fenster
```

1. **Capture:**
   - macOS nutzt ScreenCaptureKit (`SCStream`) mit einem Filter, der die
     Overlay-Fenster und alles über Fensterebene 0 (Menüleiste, Dock, Menüs,
     Dialoge) ausschließt; der Filter folgt der Fensterliste.
   - Windows nutzt `Windows.Graphics.Capture`, die Overlay-Fenster tragen
     `WDA_EXCLUDEFROMCAPTURE`.
   - Das Popup von Inspector Rust bleibt im Bild und wird retro mitgezeichnet.
   - Der Cursor wird nie mit erfasst, weil eine erfasste Kopie dem echten
     Cursor nachlaufen würde.
2. **Auflösung:** macOS erfasst in Zellauflösung (Bildschirm / Pixelgröße).
   Nur „feine Pixel" fürs aktive Fenster oder die Lupe mit Fokus-Pixeln
   brauchen volle Auflösung; die Zellen bildet dann der Shader. Eine Einstellungsänderung
   stellt den laufenden Stream um, ohne Neustart. Windows erfasst immer in
   voller Auflösung, weil die Windows-Capture-API nicht skalieren kann.
3. **Weitergabe ohne Kopie:**
   - macOS: IOSurface → `CVMetalTextureCache` → `MTLTexture`.
   - Windows: Die Capture-Oberfläche wird in eine eigene D3D11-Textur
     kopiert, weil der Frame-Pool seine Oberflächen wiederverwendet.
4. **Shader:**
   - macOS: `retro/shader.metal`. Windows: `retro/shader.hlsl`, eine 1:1-Übersetzung.
   - Die Zelle wird auf das globale Raster gesnappt und an einer Texelmitte
     abgetastet. Danach kommen der Bayer-Versatz, die Farbreduktion,
     Rahmen und Sprite sowie zuletzt Scanlines, Vignette und Krümmung.
   - 8-Bit sucht die nächste Palettenfarbe mit luminanzgewichteter Distanz
     (Rec. 601), 16-Bit quantisiert jeden Kanal.
5. **Fenster:**
   - macOS: randlos, click-through, **Ebene 1 — unter Menüleiste, Dock,
     Menüs und Dialogen**, auf allen Spaces, `sharingType = none`,
     `setCanHide:NO`. Beim Zurückweichen Deckkraft 0 sofort, zurück per
     Animator-Einblendung.
   - Windows: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST |
     WS_EX_NOACTIVATE` mit einer DirectComposition-Swapchain.

**Referenzimplementierung:**
- Die ganze Farb-, Dither- und Zellenlogik gibt es zusätzlich als reine
  Rust-Funktionen (`retro/mod.rs`). Sie rendern die Panel-Vorschau und sind
  die Vorgabe für die Shader.
- Ein Test rendert feste Testbilder offscreen durch den echten
  Metal-Shader und vergleicht sie Pixel für Pixel mit der Referenz
  (`shader_matches_the_cpu_reference`, auf einem Mac:
  `cargo test -p inspector-rust-core --lib shader_matches -- --ignored`).
- Er hat beim Bau zwei echte Abweichungen gefunden:
  - eine doppelte Rundung in der 16-Bit-Quantisierung,
  - das Abtasten genau auf einer Texelgrenze bei gerader Zellgröße.

**Vorschau:**
- Ein kleiner, eigener Capture-Stream des Monitors unter dem Cursor
  (480 px breit, höchstens 10 fps), gerendert mit der Referenzimplementierung.
- Er läuft nur, solange das Panel sichtbar ist; beim Verstecken des Popups
  stoppt er.

## Sicherheit und Akku

- Kein Frame wird gespeichert, protokolliert oder verschickt; es gibt keinen
  Netzwerkzugriff.
- Das Overlay stoppt bei Bildschirmsperre, Ruhezustand, Benutzerwechsel und
  bei Capture- oder Darstellungsfehlern, jeweils mit einer kurzen Meldung.
- Bei Monitor-Hot-Plug, Auflösungs- oder Skalierungswechsel wird es neu
  aufgebaut.
- Ändert sich der Bildschirm nicht, liefert ScreenCaptureKit keine Frames, und
  es wird nichts neu gezeichnet. Während des Zurückweichens wird ebenfalls
  nichts gezeichnet. Nur Lupe und Sprite-Cursor zeichnen bei
  Mausbewegung neu.

## Berechtigungen

- **macOS:**
  - Bildschirmaufnahme ist für Overlay und Vorschau Pflicht. Das Panel zeigt
    bei fehlender Erlaubnis einen Link zu den Systemeinstellungen.
  - Bedienungshilfen braucht nur die Ausnahme fürs aktive Fenster.
  - Solange das Overlay läuft, zeigt macOS sein Aufnahme-Symbol. Neuere
    macOS-Versionen fragen bei Bildschirmaufnahme-Apps regelmäßig erneut nach
    der Erlaubnis.
- **Windows:** keine Berechtigungen nötig.

**Windows-Stand der Bedienbarkeits-Regeln:** Löcher fürs aktive Fenster und
die Lupe (Shader) sind umgesetzt. Zurückweichen beim Tippen und das Freilassen
der Taskleiste fehlen dort noch — das Overlay ist weiter `WS_EX_TOPMOST`.

**Grenzen:** DRM-geschützte Videos (Streaming-Dienste) kommen im Capture
schwarz an und bleiben unter dem Overlay schwarz.

## Gemessene Performance

Gemessen am 2026-10-04 auf einem **Apple M1 Pro**, 1496×967 pt @2×, 60 fps.
Gezählt ist die CPU-Last des gesamten App-Prozesses (Mittel über 10 s per
`ps`).

| Lage | CPU |
|---|---|
| Overlay aus | 0,0 % |
| „Show“, Bildschirm ändert sich ~56× pro Sekunde | 6,7 % |
| „Alltag“, Bildschirm ändert sich ~56× pro Sekunde | 7,0 % |
| Bildschirm unverändert | keine Frames, keine Zeichenvorgänge |

Das **Budget von 5 % bei 60 fps wird damit verfehlt.** Laut Profil
(`sample`) entfällt der Großteil auf die Systemseite: ScreenCaptureKit-Zustellung,
Metal-Commit und `nextDrawable`. Unsere eigene Rechnung pro Frame ist klein.
Mit dem FPS-Limit 30 halbiert sich die Zahl der Frames; einen eigenen Messwert
dafür gibt es noch nicht.

Die **Latenz** beträgt etwa einen Frame: ScreenCaptureKit liefert mit diesem
Verzug, gezeichnet wird sofort im Callback. Der echte Mauszeiger ist davon
nicht betroffen, er liegt über dem Overlay.

Windows ist nicht gemessen; es gab keinen Windows-Rechner.

## Code

| Datei | Inhalt |
|---|---|
| `core/rust-lib/src/retro/mod.rs` | Paletten, Quantisierung, Bayer, Zellen/Regionen, Referenz-Renderer |
| `retro/config.rs` | Einstellungsmodell, Bereiche je Modus, Presets, Persistenz |
| `retro/frame.rs` | Monitor-Geometrie, Punkt↔Pixel, Capture-Größe, GPU-Uniform-Layout |
| `retro/font.rs` | 5×7-Pixelschrift |
| `retro/shader.metal`, `retro/shader.hlsl` | GPU-Shader |
| `retro/macos.rs`, `retro/macos_focus.rs` | macOS: Capture, Metal, Fenster, AX-Fokus, Lebenszyklus |
| `retro/windows.rs` | Windows: WGC, D3D11, DirectComposition, WinEvent |
| `retro/control.rs` | Start/Stopp/Umschalten für IPC, Hotkeys, Tray, CLI |
| `core/frontend/src/lib/retro.ts` | Parser, Zeilenmodell, Bereiche |
| `core/frontend/src/lib/retro-palettes.json` | Paletten (gemeinsame Quelle für Rust und Frontend) |
| `core/frontend/src/components/RetroPanel.tsx` | Panel mit Vorschau |
