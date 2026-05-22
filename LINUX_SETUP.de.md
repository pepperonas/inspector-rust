# Inspector Rust — Ubuntu / Linux Einrichtung

Dieses Verzeichnis ist ein **eigenes Git-Repository** (nicht der übergeordnete `cursor-Projekts`-Ordner). Quelle: [Leviticus-Triage/inspector-rust-forked----IT-Security-Mod-](https://github.com/Leviticus-Triage/inspector-rust-forked----IT-Security-Mod-).

## Einmalige Systemabhängigkeiten

Im Terminal (Passwort für `sudo` erforderlich):

```bash
cd /mnt/docker-ssd/cursor-Projekts/inspector-rust
bash scripts/install-linux.sh
source "$HOME/.cargo/env"
```

Das Skript installiert u. a. WebKit/GTK, OpenSSL-Headers, `scrot`, Tesseract, Node 20, pnpm und Rust stable.

## Entwicklung starten

```bash
pnpm dev:linux
```

Globaler Shortcut: **Ctrl+Shift+V** (Clipboard-Popup).

## Release bauen

```bash
pnpm build:linux
```

Ergebnis:

- Binary: `target/release/inspector-rust`
- Installer: `target/release/bundle/deb/InspectorRust_*_amd64.deb`

Installation:

```bash
sudo dpkg -i target/release/bundle/deb/InspectorRust_*_amd64.deb
```

## Daten & Verschlüsselung

- Datenbank: `~/.local/share/InspectorRust/history.db`
- Schlüssel: GNOME Keyring / Secret Service (Fallback: Schlüsseldatei mit Modus 0600)

## Einschränkungen unter Linux

- **OCR**: Tesseract (`tesseract-ocr`); Qualität abhängig von installierten Sprachpaketen
- **Bereichsauswahl**: X11 → `scrot`; Wayland → `grim` + `slurp`
- **Eyedropper**: noch nicht implementiert
- **Text-Expander in-place**: nur Zwischenablage/Tastatur-Fallback (kein AT-SPI)

Details: [linux/README.md](./linux/README.md)
