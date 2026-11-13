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
`inspector-rust-backup-YYYYMMDD-HHMMSS--<id>-<Mac>.json` (unverschlüsselt) bzw.
`…--<id>-<Mac>.enc.json` (verschlüsselt, AES-256-GCM, Schlüssel per Argon2id
aus deinem Passwort). `<id>` sind die ersten 8 Hex-Zeichen der Geräte-ID dieses
Macs, `<Mac>` der Computername (nur zur Anzeige).

### Mehrere Macs im selben Ordner (v0.243.0)

Vor v0.243.0 hießen die Dateien nur nach dem Zeitpunkt, und jeder Mac behielt
„die neuesten N" **aller** Dateien im Ordner — zwei Macs im selben Ordner
löschten sich gegenseitig ihre Stände weg. Jetzt löscht jeder Mac **nur Dateien
mit seiner eigenen ID**. Dateien im alten Format (ohne Geräteteil) bleiben
wiederherstellbar und werden **nie** automatisch gelöscht, weil nicht mehr
erkennbar ist, welchem Mac sie gehören; aufräumen bei Bedarf von Hand. Die
Wiederherstellungsliste zeigt je Stand „dieser Mac", den Computernamen oder
„älteres Format".

### API-Schlüssel und Zugangstoken

Die Einstellungen im Backup enthalten auch API-Schlüssel und Token (Wetter,
PageSpeed, Hue, cue-Sync) — bewusst, damit eine Wiederherstellung vollständig
ist. Unverschlüsselt liegen sie deshalb (wie die 2FA-Geheimnisse) im Klartext
im Ordner; die Einstellungen weisen darauf hin. Für Cloud-Ordner:
verschlüsseln. Beide enden auf `.json`, also lädt
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
**verschlüsselt auf diesem Mac** (`secrets.json` im Datenordner, AES-256-GCM mit dem
Datenbankschlüssel; seit v0.262.0 nicht mehr im Schlüsselbund, damit macOS nicht nach
jedem Update nach dem Login-Passwort fragt), nie in der Settings-Tabelle und nie in einer
Sicherung. ⚠️ **Ohne dieses Passwort ist die Sicherung wertlos** —
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
