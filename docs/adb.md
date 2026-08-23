# `adb` — Android-Gerät steuern

Der Popup-Companion zur Desktop-App **ADBOSS**: ein per USB oder WLAN
verbundenes Android-Gerät sehen und steuern, ohne die große App zu starten.
`adb` in die Suchleiste tippen, Enter — das Panel öffnet in der Preview-Spalte.

## Voraussetzungen (einmalig)

1. **adb auf dem Mac:** `brew install android-platform-tools`
   (fehlt es, zeigt das Panel eine Install-Karte).
2. **USB-Debugging auf dem Handy:** Einstellungen → Über das Telefon →
   7× auf „Build-Nummer" tippen → Entwickleroptionen → **USB-Debugging** an.
3. Beim ersten Verbinden erscheint auf dem Handy „USB-Debugging zulassen?" —
   bestätigen (Haken bei „immer erlauben"). Solange das aussteht, zeigt das
   Panel „nicht autorisiert".

## Die fünf Ansichten

| Ansicht | Aufruf | Inhalt |
|---|---|---|
| **Info** | `adb` | Live-Dashboard (5-s-Poll): Modell, Android-Version, SDK, Auflösung, Uptime · Akku mit Temperatur/Spannung · RAM- und Speicher-Balken · WLAN-SSID/IP/RSSI. Dazu **Screenshot** und **Bildschirmaufnahme**. |
| **Steuern** | Chip im Panel | WLAN/Bluetooth/Flugmodus/Nicht-stören (an/aus), Helligkeit (0–255), Medien-Lautstärke (0–15), Display wecken/aus/sperren. |
| **Remote** | `adb remote` | Navigationstasten (Home/Back/Recents/Power/Volume…), D-Pad mit OK/Enter, Text senden, Tap und Swipe auf Koordinaten. |
| **Apps** | `adb apps` | Installierte Pakete durchsuchen (System-Apps zuschaltbar), Start / Stoppen / Daten löschen / Deinstallieren — destruktives mit nativer Bestätigung. |
| **WLAN** | `adb wifi` | USB-Gerät auf TCP/IP schalten (`adb tcpip 5555`), IP wird automatisch erkannt und verbunden — danach kann das Kabel ab. Alternativ direkte `ip:port`-Verbindung, Trennen pro Gerät. |

## Screenshot & Aufnahme

- **Screenshot** (`exec-out screencap -p`) landet direkt im **Mac-Clipboard
  und im IR-Verlauf** — sofort überall einfügbar. Der Screenshot-Sound folgt
  der Settings-Auswahl.
- **Aufnahme** startet `screenrecord` auf dem Gerät; „Stoppen" beendet sauber,
  zieht die MP4 nach `~/Downloads/android-record-<ts>.mp4` und zeigt sie im
  Finder.

## Grenzen (ehrlich)

- `input text` kann nur **ASCII** zustellen — Umlaute/Emoji lehnt das Panel
  mit Hinweis ab, statt kaputte Zeichen zu senden.
- Bluetooth-Toggle via `svc bluetooth` funktioniert nicht auf jeder
  Android-Version (herstellerabhängig eingeschränkt).
- Logcat-Viewer, Bluetooth-HCI-Analyzer, Datei-Browser und Settings-Browser
  bleiben bewusst in **ADBOSS** — dafür braucht es ein volles Fenster.
- Alle Werte werden vor jedem Shell-Aufruf strikt validiert (Paketnamen,
  Keycodes, Koordinaten-Ranges, IP:Port) — Shell-Metazeichen kommen nie durch.

## Sicherheit

Jede Geräte-Interaktion läuft über die von ADBOSS erprobten Kommandoformen.
Text-Eingaben werden POSIX-korrekt gequotet (`'\''`-Form — Backslash-Escaping
existiert in Single-Quotes nicht und wäre eine Injection-Lücke gewesen;
Sicherheitsreview 2026-08-24, per Test gepinnt).
