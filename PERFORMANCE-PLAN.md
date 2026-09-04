# Performance-Plan — Inspector Rust (Stand 2026-09-05, v0.165.0)

Ziel: die App **spürbar und messbar** schneller/leichter machen, **ohne eine
einzige Funktion zu verlieren**. Jede Maßnahme trägt ihren Befund (Datei:Zeile
oder Messwert), den erwarteten Gewinn, das Risiko und die Verifikation. Was
sich nicht messen lässt, wird nicht behauptet.

## 0. Ausgangslage — gemessen, nicht geraten

Live am 05.09. (M1 Pro, Uptime 2 h 53 min, Popup versteckt):

| Größe | Ist | Bewertung |
|---|---|---|
| RSS | **29 MB** (top: 49 MB) | ✓ unter dem „< 50 MB"-Versprechen |
| Idle-CPU | **0,4–1,0 %** | ✗ für eine Tray-App zu viel — siehe A2 |
| Threads | **39** | 12 benannte Dauer-Threads + Tauri/WebKit |
| Startup bis „alle Monitore scharf" | **~1,2 s** (17:16:27.28 → 28.52), davon **430 ms bis „db at"** | ✗ DB-Öffnen dominiert — siehe A1 |
| Popup-Öffnen nativ | 17–38 ms (Debug-Log, v0.107.0) + CRT ≤ 250 ms | ✓ gepinnt |
| history.db | **331 MB**, Freelist **32 946 von 84 832 Seiten (39 %)** | ✗ ~130 MB toter Raum — siehe A1 |
| davon `entries.content_data` | 152 MB, **214 Bild-Clips = 138 MB** | ✓ `list_slim` hält sie aus dem Popup-Pfad |
| `track_events` | **162 136 Zeilen** | ✓ Retention läuft; Index-Prüfung offen — B4 |
| Binary | **60 MB** (ort statisch ~40 MB, u2netp 4,5 MB) | Disk/Download, **nicht** RSS (mmap, on demand) — C1 |
| Frontend App-Chunk | **971 KB**, 45 Panels eager, 0 lazy in App.tsx | ✗ Startup-Parse/JIT — A4 |
| App.tsx | 5 275 Zeilen, 78 Effekte, 43 Memos, 109 States, **12 Effekte hängen an `query`** | Pro Tastendruck — A5/B6 |
| Release-Profil | lto=true, codegen-units=1, opt-level=3, strip=debuginfo | ✓ bereits maximal; `panic=abort` ist KEIN Hebel (4× `catch_unwind` im Code) |

## 1. Leitplanken

1. **Messen vor und nach jeder Maßnahme**, dieselbe Methode (Abschnitt D).
   Ein Gewinn ohne Zahl ist keiner.
2. **Kein Funktionsverlust, keine Verhaltensänderung** außer Latenz/Ressourcen.
   Wo ein Cache Verhalten ändern könnte (Snippets), wird die Invalidierung
   getestet, nicht angenommen.
3. **Budgets werden zu Tests** (das CRT-Muster: `md3-motion.test.ts` pinnt
   ≤ 250 ms) — sonst wächst alles zurück.
4. **Ehrlich über Nicht-Hebel**: Binary-Größe ≠ Speicher (mmap), Threads ≠ CPU
   (parkende Threads kosten nichts), `panic=abort` bricht `catch_unwind`.
5. Eine Maßnahme pro Commit; Mutationsprobe für jeden neuen Pin.

## 2. Maßnahmen nach Wirkung × Risiko

### A — Sofort (hoher Gewinn, geringes Risiko)

**A1 · DB-Wartung: VACUUM + Auto-Maintenance** — *Befund:* 39 % Freelist
(≈ 130 MB) in einer 331-MB-Datei; das DB-Öffnen beim Start dauert 430 ms
(`lib.rs:184`). *Maßnahme:* einmaliges `VACUUM` (Wartungspfad im Backup-
Modul oder Settings → Clipboard history „DB verdichten"), dann
`PRAGMA auto_vacuum=INCREMENTAL` + `incremental_vacuum(N)` im nächtlichen
Prune-Pfad des Stats-Collectors; `wal_autocheckpoint` prüfen (WAL 3,9 MB ist
ok). *Gewinn:* −130 MB Disk, kürzeres Öffnen, schnellere Backups/Prune.
*Risiko:* VACUUM braucht temporär 2× Platz und sperrt die DB (Sekunden) —
nur bei verstecktem Popup, mit Busy-Timeout. *Verifikation:*
`PRAGMA freelist_count` < 2 %, Start-bis-„db at" gemessen.

**A2 · Gesten-Settle-Ticker ereignisgesteuert** — *Befund:*
`gestures/macos.rs:228,254` — `ir-gestures-tick` schläft **dauerhaft 24 ms**
(~42 Wakeups/s) und pollt `RUNNING`, auch wenn seit Stunden kein Finger auf
dem Pad liegt; der wahrscheinlichste Anteil der 0,4–1 % Idle-CPU.
*Maßnahme:* Ticker parkt (`Condvar`/`park`) und wird vom Frame-Callback nur
geweckt, während ein Tap-Cluster offen ist (`cluster_open`); nach dem
Finalisieren parkt er wieder. Erkennung, Zeitkonstanten (`TAP_SETTLE_MS`
160, `TICK_MS` 24) und Verhalten bleiben identisch — nur der Leerlauf
verschwindet. *Gewinn:* Idle-Wakeups ~42/s → 0. *Risiko:* gering; der
Stop-Pfad (`join`) muss den parkenden Thread wecken (Test: start/stop-Zyklus
hängt nicht). *Verifikation:* `top`/`powermetrics` Idle-CPU 60 s vorher/
nachher; Tap-Latenz unverändert (bestehende 14 Recognizer-Tests).

**A3 · Timesheet-Heartbeat entkoppeln** — *Befund:* `tracking/mod.rs:32,488`
— jeder 1,5-s-Tick schreibt `touch_event` in die DB = **2 400 WAL-Writes pro
Stunde**, Tag für Tag, nur damit ein Absturz das offene Ereignis auf ≤ 1,5 s
genau beendet. *Maßnahme:* Heartbeat alle 10 s (Absturz-Granularität 10 s
statt 1,5 s — für eine Zeiterfassung irrelevant) UND nur, wenn sich
`ended_at` real bewegt; Fokus-/Idle-Wechsel schreiben weiter sofort.
*Gewinn:* −85 % DB-Writes im Dauerbetrieb, weniger WAL-Churn (A1 wirkt
länger). *Risiko:* gering; Pin `resume_if_active`-Tests bleiben grün.
*Verifikation:* Write-Zähler über 10 min (SQLite `total_changes`).

**A4 · Frontend-Panels lazy laden** — *Befund:* `App.tsx` importiert **45
Panel-Komponenten eager**, 0 lazy; der App-Chunk hat 971 KB und wird beim
Start des versteckten Popups komplett geparst/JIT-kompiliert; `main.tsx` macht
es für die Aux-Fenster längst richtig (`React.lazy`, v0.84.228).
*Maßnahme:* die Preview-Panels (`*Panel.tsx`, Spiele, Overlays,
`SettingsPanel` 5 640 Zeilen, `TimesheetPanel`, `ScreenshotEditor`,
`EqualizerVisualizer`, `BpmDetector`, …) per `React.lazy` + `Suspense`
laden; `HistoryList`/`HistoryItem`/`PreviewPanel`/`SearchBar`/`Footer`
bleiben eager (Öffnungspfad). *Gewinn:* Start-Parse ≈ 970 → ~300 KB,
weniger JS-Heap; kein Einfluss auf Hotkey→Popup (Chunk lädt vom lokalen
Asset-Protokoll in ms, und das Popup existiert vor dem ersten Hotkey).
*Risiko:* erstes Öffnen eines Panels lädt nach (lokal, ~1 Frame); die
Show-while-typed-Panels müssen ihren Auto-Exit-Effekt nicht neu bewerten
(`PANEL_KINDS`-Kanonisierung unverändert). *Verifikation:* `dist/assets`-
Größen (Delta-Tabelle wie in v0.163.0), `performance.now()`-Marke bis
„shell mounted" im Startlog.

**A5 · `findSnippets` ohne Entschlüsselung pro Tastendruck** — *Befund:*
`snippets.rs:197–234` — die Body-Suche entschlüsselt AES-GCM-Bodies, bis 10
Treffer stehen; bei einer Query ohne Titel-/Kürzel-Treffer sind das **alle
289 Bodies bei jedem Anschlag**, über IPC. *Maßnahme:* ein prozessweiter
Klartext-Suchindex der Snippets (`abbreviation`/`title`/`body` lowercased),
aufgebaut beim Start und von JEDEM Snippet-CRUD + Sync + Backup-Import
invalidiert — genau die Stellen, die heute `auto_expand::rebuild_table`
rufen (ein Hook, keine neue Liste). *Gewinn:* Tastendruck-Latenz auf der
Snippet-Seite ~konstant statt O(Snippets) Krypto. *Risiko:* Invalidierung
vergessen = veraltete Treffer → ein Test je Schreibpfad (create/update/
delete/upsert/sync/import) pinnt „Index sieht die Änderung". *Verifikation:*
Zeit `find_snippets` mit 289 Snippets, Query ohne Treffer, vorher/nachher.

### B — Mittel (klarer Gewinn, mehr Umbau oder Messbedarf)

**B1 · Startup-Reihenfolge** — *Befund:* 430 ms bis „db at" umfassen
Keychain-Zugriff (`crypto::init`, `lib.rs:164`) + Öffnen der 331-MB-DB +
Lazy-Migrationen (12 DDL/PRAGMA in `db.rs`); danach synchron Seed-Prüfung,
Timesheet-Resume, Hotkeys. *Maßnahme:* nach A1 neu messen; dann alles, was
das Popup nicht braucht (Stats-Collector `lib.rs:449`, Sync-Worker, Device-
Sync, Brightness-Restore-Thread) hinter den ersten Frame schieben; das
`ALTER TABLE`-Probing durch eine gespeicherte Schema-Version ersetzen (ein
Read statt N `PRAGMA table_info`). *Gewinn:* geschätzt −200–400 ms bis
„bereit"; **erst nach Messung behaupten**.

**B2 · Popup-Öffnen: History nur nachladen, wenn sich etwas änderte** —
*Befund:* `window-shown` refresht Snippets + Wakelock, und
`useClipboardHistory` holt beim Öffnen unbedingt die Liste (`list_slim`:
1 000 Zeilen entschlüsseln + JSON + IPC), obwohl seit dem letzten Öffnen oft
kein `clipboard-changed` kam (das Visibility-Gate verwirft Events nur, es
merkt sie sich nicht). *Maßnahme:* Dirty-Flag — `clipboard-changed` im
versteckten Zustand setzt es, `window-shown` refresht nur bei gesetztem
Flag. *Gewinn:* der häufigste Öffnungsfall ohne DB/Krypto/IPC. *Risiko:*
ein verpasstes Event = veraltete Liste → das Flag wird IMMER gesetzt, wenn
das Event kam, und zusätzlich bei jedem eigenen Upsert-Pfad; Test mit
Fake-Events.

**B3 · `combined` in stabile Teil-Memos zerlegen** — *Befund:* das große
`useMemo` in `App.tsx` hängt an ~40 Dependencies; jede Panel-State-Änderung
(z. B. `navInstant` alle 160 ms bei gehaltener Pfeiltaste, Toast-Timer)
rechnet die komplette Listenassemblierung neu (Clip-Filter über 1 000
Einträge, Snippet-Map, alle Sub-Rows). *Maßnahme:* Clips+Snippets als
eigenes Memo (nur `query`/Daten), Command-Rows als zweites, finale
Konkatenation billig; Reihenfolge-Invarianten (Custom-Commands vor
`appEntry`, Settings-Rows danach) bleiben in EINER Funktion und werden
gepinnt. *Gewinn:* weniger Re-Render-Arbeit pro Anschlag; *Risiko:* die
Reihenfolge-Invariante ist genau das, was man dabei bricht — die
bestehenden Row-Order-Tests sind der Wächter.

**B4 · Timesheet-Abfragen mit 162 k Zeilen** — *Maßnahme:* `EXPLAIN QUERY
PLAN` für `day_report`/`range_report`/`cleanup_day`/`slots`; fehlende Indizes
auf `(session_id, started_at)`/`started_at` ergänzen (lazy `CREATE INDEX IF
NOT EXISTS`); Retention nach A1 verdichten. *Gewinn:* Timesheet-Tab-Öffnen
und Slots-Berechnung; *Verifikation:* Zeit `track_get_day` vorher/nachher.

**B5 · Stats-Collector schlanker** — *Befund:* `system_stats.rs:104–200` —
`gather()` erzeugt alle 60 s neue `Networks`/`Disks`/`Components`-Listen
(statfs je Mount, SMC-Reads) und schläft 200 ms für das CPU-Fenster; der
Hintergrund-Sammler braucht aber nur CPU %, RAM %, Netz-Bytes, Watt, Temp,
Akku. *Maßnahme:* eine wiederverwendete `System`-Instanz + `gather_core()`
ohne Disks/Components-Neuaufbau für den 60-s-Sammler; das vollständige
`gather()` bleibt für das sichtbare `stats`-Panel. *Gewinn:* weniger Syscalls
je Minute; klein, aber dauerhaft.

**B6 · PreviewPanel-Re-Renders** — *Befund:* 2 286 Zeilen, rendert bei jeder
Selektionsänderung komplett neu (Crossfade-Effekt liest Refs). *Maßnahme:*
`React.memo` auf die inneren Branch-Komponenten (Clip/Command/Snippet/…),
teure Ableitungen (`detectSmartActions`, Markdown/Inline-MD) memoisieren.
*Verifikation:* React-Profiler-Commit-Zeit pro ↓-Druck.

**B7 · `clipboard-changed`-Bursts koaleszieren** — viele IPCs emittieren das
Event einzeln (Upsert-Familie); bei Batch-Operationen (Backup-Restore,
Clear) kommt es N-fach → N Refetches. *Maßnahme:* Emit am Ende der
Operation, im Frontend zusätzlich ein 30-ms-Debounce im Hook.

### C — Größe & Verteilung (ehrlich: kein Laufzeitgewinn)

**C1 · ONNX Runtime optional** — *Befund:* ort statisch ≈ 40 MB + u2netp
4,5 MB im Binary, genutzt nur vom Freistellen (`cut_out`). Die Seiten sind
**mmap'd und werden erst beim ersten Cut-out eingeblendet** — RSS 29 MB
beweist es. *Maßnahme (optional):* Cargo-Feature `cutout` (Default an) oder
Download-on-first-use; senkt DMG/Download 60 → ~20 MB. *Kein* Startup- oder
RAM-Hebel — nicht als solcher verkaufen.

**C2 · Frontend-Bundle nach A4** — `totp-icons` (4,7 MB) und figlet-Fonts sind
bereits lazy; nach A4 bleibt ein Audit der verbleibenden Eager-Deps
(`lucide-react`-Icons einzeln importieren ✓ bereits, `qrcode-generator`
lazy).

**C3 · Was bewusst NICHT geändert wird:** `panic = "abort"` (4× `catch_unwind`
— u. a. der Metal-Draw-Schutz in `edr.rs`), `opt-level = "z"` (Geschwindigkeit
vor Größe), ein zweiter Prozess für Hintergrund-Monitore (Komplexität ohne
Messbeleg), Reduktion der 39 Threads um ihrer selbst willen (parkende
Threads kosten ~0; die Frage ist Wakeups, siehe A2).

### D — Messbarkeit dauerhaft

1. **`scripts/perf-probe.sh`**: liest aus dem Log Start→„db at"→„armed",
   misst 60 s Idle-CPU (`top -l`), RSS, Thread-Zahl, `PRAGMA freelist_count`/
   `page_count`, `dist/assets`-Größen; gibt eine Zeile JSON aus — vor/nach
   jeder Maßnahme, Ergebnis in den Commit-Text.
2. **Popup-Latenz**: `RUST_LOG=debug` Phasen aus `show_and_position` (bereits
   geloggt) über 20 Öffnungen mitteln; Budget bleibt ≤ 250 ms gesamt.
3. **Pins als Tests**: (a) `ir-gestures-tick` hat keine bedingungslose
   `sleep`-Schleife (Quelltext-Pin, wie die CSS-Pins); (b) App-Chunk ≤ 400 KB
   (Build-Skript-Check in `check.sh`, analog zu gen-docs); (c) Snippet-Index
   invalidiert bei jedem Schreibpfad (Unit-Tests); (d) Timesheet-Heartbeat-
   Intervall ≥ 10 s (Konstanten-Pin).
4. **Vitest/React-Profiler-Probe** für den Tastendruck-Pfad: Render-Count
   von `HistoryList` pro `query`-Änderung (Ziel: 1).

## 3. Reihenfolge (jede Etappe = eigener Commit + Vorher/Nachher-Zahl)

| Etappe | Maßnahmen | Wozu zuerst |
|---|---|---|
| 0 | D1 Probe-Skript | ohne Baseline kein Beweis |
| 1 | A1 VACUUM/Auto-Vacuum, A3 Heartbeat | größter Disk-/Write-Hebel, trivial |
| 2 | A2 Gesten-Ticker | der Idle-CPU-Hebel |
| 3 | A5 Snippet-Index, B2 Dirty-Flag | Tastendruck + Öffnungspfad |
| 4 | A4 Lazy-Panels | Startup-Parse, Speicher |
| 5 | B1, B3, B6, B7 | nach Messung, Reihenfolge nach Ausbeute |
| 6 | B4, B5 | Dauerbetrieb-Hygiene |
| 7 | C1 (optional, Nutzerentscheid) | Download-Größe |

Erwartung, ehrlich gerundet: Idle-CPU ~0,5 % → nahe 0; DB 331 → ~200 MB;
Start-Parse des Popups −2/3; Snippet-Tastendruck ohne Krypto; Öffnen ohne
DB-Roundtrip im Normalfall. Der Hotkey→Paste-Pfad ist heute schon unter
200 ms und bleibt das gepinnte Budget.

## 4. Offene Punkte außerhalb dieses Plans

- **boom routet auf Loopback-Geräte** (Befund 04.09., 19:33: „restoring
  remembered output 'BlackHole 2ch'" → Stille). Kein Performance-Thema, aber
  ein Korrektheits-Bug: Loopback-/Virtual-Geräte dürfen nie Brücken-Ziel
  sein. Separat fixen.
