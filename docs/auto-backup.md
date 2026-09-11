# Automatische Backups (v0.170.0)

Sichert den kompletten App-Zustand in einem festen Intervall in einen Ordner
deiner Wahl — z. B. einen Google-Drive- oder iCloud-Ordner —, optional
verschlüsselt, mit begrenzter Anzahl aufbewahrter Stände, und aus den
Einstellungen wieder einspielbar. Einzustellen unter **Settings → Automatische
Backups**. Standardmäßig **aus**.

## Was gesichert wird

Dasselbe Voll-Backup wie der manuelle „Export": **Einstellungen, Snippets
(inkl. Gruppen), Notizen, 2FA/TOTP** und — abschaltbar — der
**Zwischenablage-Verlauf**; die **Zeiterfassung** ist ein eigener Schalter
(Standard aus). Einstellungen, Snippets, Notizen und 2FA sind immer enthalten:
ein Backup, das deine Konfiguration oder 2FA nicht wiederherstellen kann, ist
keins.

> ⚠️ **Warum eine Datei und nicht die rohe SQLite-DB kopiert wird:** Die
> Datenbank ist at-rest mit einem Schlüssel aus dem Schlüsselbund dieses Macs
> verschlüsselt — eine reine Kopie wäre ohne genau diesen Schlüssel wertlos und
> nicht auf ein anderes Gerät übertragbar. Der Export entschlüsselt einmal
> sauber und verschlüsselt dann mit **deinem** Backup-Passwort. So lässt sich
> ein Stand auf jedem Gerät wiederherstellen.

## Format

Jeder Snapshot ist **eine Datei** im bewährten Backup-Format:
`inspector-rust-backup-YYYYMMDD-HHMMSS.json` (unverschlüsselt) bzw.
`inspector-rust-backup-YYYYMMDD-HHMMSS.enc.json` (verschlüsselt, AES-256-GCM,
Schlüssel per Argon2id aus deinem Passwort). Beide enden auf `.json`, also lädt
sie auch der bestehende **„Import"-Knopf** — es gibt zwei Wege zur
Wiederherstellung.

## Intervall, Aufbewahrung, „nur bei Änderung"

- **Intervall** (Minuten): wie oft geprüft wird. Standard 60, Minimum 1.
- **Geschrieben wird nur bei Änderung.** Ein Voll-Export wird gehasht (über den
  reinen Inhalt, ohne den Export-Zeitstempel und ohne die eigenen
  Heartbeat-Einstellungen); ist er identisch mit dem letzten geschriebenen
  Stand, wird **keine** neue Datei angelegt. Sonst stapelten sich stündlich
  byte-identische Mehr-MB-Dateien in deinem Cloud-Ordner.
- **Stände behalten** (N): die neuesten N Snapshots bleiben, ältere werden
  gelöscht — **nur unsere eigenen Dateien**, fremde im Ordner nie.

## Verschlüsselung

Optional, aber bei einem Cloud-Ordner dringend empfohlen. Das Passwort liegt
**nur im Schlüsselbund** dieses Macs, nie in einer Datei oder der
Settings-Tabelle. ⚠️ **Ohne dieses Passwort ist die Sicherung wertlos** —
notiere es zusätzlich in einem Passwortmanager. 2FA-Geheimnisse werden
mitgesichert; bei **ausgeschalteter** Verschlüsselung liegen sie im Klartext im
Ordner (die App warnt in diesem Fall deutlich).

## Wiederherstellen

Unter „Wiederherstellen…" listet die App die Stände im Ordner (neueste zuerst,
mit Datum, Größe und Schloss-Symbol bei verschlüsselten). Zwei Modi:

- **Zusammenführen** (Standard): ergänzt/aktualisiert nur, löscht nie. Sicher.
- **Alles ersetzen**: stellt exakt diesen Stand her — seit dem Backup
  Hinzugefügtes geht verloren (mit Rückfrage).

Verschlüsselte Stände verlangen das Passwort. Ein falsches Passwort wird
abgewiesen, nichts wird angetastet.

## Grenzen

Es gibt bewusst nur den **Papierkorb-freundlichen** Weg über Dateien im Ordner;
keine Versionierung über die Aufbewahrungsgrenze hinaus und keine
Off-Site-Kopie über den gewählten Ordner hinaus — das erledigt der Cloud-Dienst,
in dessen Ordner du sicherst.
