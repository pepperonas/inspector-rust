# Wie der Text-Expander auf macOS *wirklich* zum Laufen kam

Eine Bug-Story über drei zusammenhängende macOS-Eigenheiten, die jeden Tauri-/Electron-App-Entwickler treffen, sobald die App synthetische Tastatureingaben braucht. Geschrieben nach der v0.43.3-Release-Reihe, in der diese Probleme nacheinander aufgepoppt sind und einzeln gefixt wurden.

Wenn du eine eigene Mac-App baust und auf TCC-Schmerz triffst, ist das hier die Karte aus diesem Wald.

---

## TL;DR

Drei Probleme, drei voneinander unabhängige Fixes — alle drei mussten zusammenkommen:

1. **Code-Signatur instabil** → TCC-Grant verlor bei jedem Rebuild seine Gültigkeit.
   Lösung: eigenes selbst-signiertes Zertifikat in einem dedizierten Keychain. Designated Requirement referenziert nur noch den **Cert-Hash**, nicht den **cdhash** des Binaries. Damit überleben TCC-Grants jeden Rebuild.

2. **`AppleScript "System Events"` erforderlich, aber nicht granted** → Inspector-Frontmost-Probe gab silent False zurück.
   Lösung: Frontmost-Check durch `Window::is_visible()` ersetzt. Keine TCC-Permission nötig, dafür akkurater für unseren Use-Case.

3. **Carbon-Hotkey-Dispatcher schluckt Keydowns** → `Alt+1` erreichte den Webview nie, sobald global registriert.
   Lösung: bei aktivem Popup das Event als Tauri-Event (`expander-hotkey-forwarded`) ins Frontend werfen, dort interpretieren. JS-Handler reagiert wie bei einem nativen Keydown.

---

## Problem 1 — TCC-Grant überlebt keinen Rebuild

### Symptom

Wir schalten in `System Settings → Privacy & Security → Accessibility` die Inspector-Rust-Permission an. Funktioniert für genau eine Session. `pnpm build:macos` + Re-Install → Permission ist plötzlich wieder weg, ohne dass wir was am Permission-Dialog geändert haben.

### Root-Cause

macOS' TCC (Transparency, Consent, Control) bindet jeden Permission-Grant an die **code signature** der App. Genauer: an ihre **Designated Requirement**.

Ein `tauri build` ohne Signing-Identität produziert eine ad-hoc-signierte Binary. Bei ad-hoc-Signing sieht die Designated Requirement so aus:

```
designated => cdhash H"fbba96093bc60791d686328c5b8d4a14a52f0579"
```

Das `cdhash` ist ein SHA-256 über die kompilierte Mach-O. Jeder Rebuild → andere Binary → anderes `cdhash` → andere Designated Requirement → macOS sieht die "neue" App als unbekannt an → TCC-Eintrag matcht nicht mehr → kein Grant.

Rust-Release-Builds sind **nicht byte-reproducible** (Timestamps, Pfade, LLVM-Nondeterminismus), also ist jeder Rebuild garantiert ein neues `cdhash`.

### Lösung

Selbst-signiertes Zertifikat in einem dedizierten Keychain. Designated Requirement bei einem echten (auch selbst-signierten) Cert:

```
designated => identifier "io.celox.inspector-rust" and
              certificate leaf = H"6bb7ca176cae61e3e3145531126b72790f8dc596"
```

Beachte: **kein `cdhash`** mehr. Stattdessen Bundle-ID + Leaf-Cert-Hash. Beides bleibt über Rebuilds hinweg stabil — Bundle-ID kommt aus dem Manifest, Cert-Hash aus dem Keychain.

`scripts/install-macos.sh` legt das Cert einmalig in `~/Library/Keychains/inspector-rust-signing.keychain-db` an, mit hardcoded Passwort (das Keychain enthält nur diesen einen Cert — es ist nichts wert außerhalb dieser Maschine, das Passwort ist kein Secret). Bei jedem Re-Install signiert das Skript mit dieser Identity.

**Warum kein Apple Developer ID?** Kostet 99 $/Jahr und ist für Open-Source-Tools / lokale Entwicklung Overkill. Selbst-signiert reicht völlig, solange du nur den **lokalen TCC-Persistenz**-Effekt willst. Was du *nicht* kriegst ohne Developer ID: Notarization-freie Distribution + Gatekeeper-Bypass für andere User. Aber für die eigene Maschine spielt das keine Rolle.

### Häufiger Stolperstein

`pnpm build:macos` bundled DMG **und** .app. Wenn die DMG-Stage failt (passiert auf manchen macOS-Versionen wegen `bundle_dmg.sh`-Quirks), wird der ganze `tauri build`-Process exit 1 — aber die .app wurde **vorher** schon gebaut. Wenn dein Install-Skript dann das Tauri-Default-`.app` aus `target/release/bundle/macos/` kopiert ohne separat zu re-signen, kriegst du eine ad-hoc-signierte Binary trotz Stable-Cert-Setup. Das war v0.41 → v0.42 unser Bug: die installierte Binary war ad-hoc obwohl das Setup korrekt war.

Fix: `install-macos.sh` ruft am Ende explizit `codesign --force --deep --sign <stable-cert> --identifier <bundle-id>` über das fertig kopierte Bundle. Egal wie verbeult `tauri build` rauskommt — am Ende steht die richtige Signatur drin.

### Verifikation

```bash
codesign -d --requirements - /Applications/InspectorRust.app
# erwartet: designated => identifier "..." and certificate leaf = H"..."
# NICHT:    designated => cdhash H"..."
```

Wenn da `cdhash` steht: Signing ist verkackt.

---

## Problem 2 — `tell application "System Events"` braucht ein eigenes TCC-Grant

### Symptom

Wir bauen einen "ist Inspector Rust gerade frontmost?"-Check, um zu entscheiden ob ein globaler Hotkey gerade in unserem eigenen Fenster ausgelöst wird (skip → unser Frontend macht's) oder von extern (run → Expander-Pipeline).

Bei dem User wo "Automation → Finder" enabled ist, aber Accessibility noch nicht granted: Alt+1 öffnet trotz Inspector-Frontmost-Check den Settings-Tab mit dem "Accessibility nicht granted"-Banner.

### Root-Cause

Unser Frontmost-Check sah so aus:

```rust
const SCRIPT: &str = r#"tell application "System Events" to get name of first application process whose frontmost is true"#;
// ... osascript ausführen, parse output, return Option<String>
```

Apple Events sind **per-Target-App** TCC-gateted. `Automation → Finder` granted heißt: Inspector Rust darf Apple Events an `com.apple.Finder` schicken. Aber `tell application "System Events"` ist ein Apple Event an `com.apple.systemevents` — eine **andere** App. Eigenes TCC-Grant, eigener Prompt, eigene Einträge in `~/Library/Application Support/com.apple.TCC/TCC.db`.

Wenn das System-Events-Grant nicht da ist, schlägt `osascript` lautlos fehl (Inspector Rust kriegt einen 1743-Error zurück, den unser Code in `None` umsetzt). Resultat: `inspector_rust_is_frontmost()` returnt False → wir denken wir sind nicht frontmost → der Gate greift nicht → Expander läuft weiter in den AX-Check → AX nicht granted → Banner.

Das Heimtückische: alles funktioniert **wenn** der User irgendwann den System-Events-Prompt geklickt hat. Auf einem frischen Mac, der nur Finder-Automation kennt, schlägt es zu.

### Lösung

`Window::is_visible()` aus der Tauri API — eine reine Window-State-Abfrage über AppKit's `NSWindow.isVisible`. Braucht **keine** TCC-Permission, weil sie nur die Window-State unseres *eigenen* Prozesses liest.

```rust
fn popup_is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(POPUP_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}
```

Funktioniert auf einem frisch entpackten App-Bundle ohne einen einzigen TCC-Klick.

### Warum das eigentlich *besser* als der Frontmost-Check ist

Der Frontmost-Check fragt: "Ist *die App* aktuell die vorderste?" Der Visible-Check fragt: "Ist *unser Popup* offen?" Für unseren Use-Case (Hotkey gefeuert → soll Expander oder In-Popup-Handler reagieren?) ist die zweite Frage akkurater:

- Inspector Rust läuft permanent im Hintergrund (Tray-App, Activation-Policy `Accessory`). Der Visible-Check ist nur dann true wenn der User unser Popup explizit offen hat. Beim Frontmost-Check müsste man definieren was "frontmost" für einen Tray-Process überhaupt heißt.
- Unser Popup hidet sich automatisch on blur. `is_visible == true` heißt also de facto "der User schaut uns gerade an + interagiert mit uns".
- Keine flakiness durch Apple-Events-Round-Trips. Reine in-Process-State-Abfrage, ~10 µs.

### Lesson

Wenn du auf macOS irgendwas brauchst was "ist meine App im Fokus" abfragt, **fragnur deine eigenen Window-Handles**. AppleScript-/NSWorkspace-/Carbon-basierte Frontmost-Probes brauchen alle entweder TCC-Grants oder haben Edge-Cases mit System-Prozessen. `NSWindow.isKeyWindow` / `NSWindow.isVisible` sind die ehrlichsten Antworten.

---

## Problem 3 — Carbon-Hotkey-Dispatcher schluckt Keydowns

### Symptom

User hat den Text-Expander auf `Alt+Digit1` (default). User öffnet unser Popup und drückt `Alt+1` mit der Intention, im pwgen-Mode-Switch das "All chars"-Mode zu triggern. Alt+2/3/4 funktionieren perfekt — Alt+1 macht *nichts*.

### Root-Cause

macOS registriert globale Hotkeys über das **Carbon Hotkey Manager API** (`RegisterEventHotKey`). Tauri's `global_shortcut`-Plugin nutzt das intern. Sobald ein Hotkey registriert ist, **swallowt** der Carbon-Dispatcher den keydown — der eigentliche Frontmost-App-Process (also auch unser eigener Webview) sieht die Keystroke nie. Das ist by-design: ein global registrierter Hotkey soll überall hin durchgreifen, nicht nur in Apps die das Event nicht anders verarbeiten.

Für uns heißt das: Sobald wir `Alt+Digit1` für den Expander registrieren, ist `Alt+1` in unserer eigenen App tot. Egal ob wir frontmost sind oder nicht, egal ob wir einen `keydown`-Listener gebunden haben — der Webview bekommt das Event nicht. Unser In-Popup JS-Handler für `Alt+Digit1 → pwgen mode "all"` läuft nie.

Alt+2/3/4 funktionieren, weil dafür keine globalen Shortcuts registriert sind. Diese keydowns flow normal durch die Cocoa-Event-Loop in den focused-Webview.

### Lösung

Wenn der globale Expander-Hotkey feuert *und* das Popup offen ist (siehe Problem 2 für den TCC-freien Visible-Check):

1. **Nicht** die Expander-Pipeline laufen lassen (sinnlos — würde in unsere eigene Suchleiste tippen)
2. **Stattdessen** ein Tauri-Event mit dem Hotkey-String emitten:

   ```rust
   if popup_is_visible(&app) {
       let _ = app.emit("expander-hotkey-forwarded", hotkey_str.clone());
       return;
   }
   ```

3. Frontend hört auf das Event und übersetzt:

   ```typescript
   useTauriEvent<string>("expander-hotkey-forwarded", (e) => {
       if (!selectedPwgen) return;
       const m = e.payload.match(/^Alt\+Digit([1-4])$/i);
       if (!m) return;
       const digit = Number(m[1]);
       const mode = ["all", "alnum", "dict", "leet"][digit - 1];
       setPwgenMode(mode);
       setPwgenSeed((s) => s + 1);
   });
   ```

Net effect: `Alt+1` fühlt sich für den User wie ein normaler keydown an — obwohl er technisch nie den Webview erreicht hat. Der JS-Handler sieht das Forwarded-Event und macht denselben State-Update wie für die nicht-geschluckten Digits 2-4.

### Symmetrie für Direct-Slot-Hotkeys

Falls der User einen Direct-Slot auf `Alt+1` gebunden hätte, würde der ebenfalls den keydown schlucken. Gleicher Fix: `register_direct_slots` emitted ebenfalls `expander-hotkey-forwarded` beim Popup-Visible-Check. Frontend bekommt's, mappt's, fertig.

### Warum nicht einfach den Expander-Hotkey deregistrieren während Popup offen ist?

Race-Condition: `window-shown`-Event fired async. User kann in der Lücke `Alt+1` drücken. Außerdem: jedes Mal die Global-Shortcut-Registry an/aus zu schalten ist heavy + kann mit anderen Hotkey-Mutationen kollidieren.

Das Event-Forward-Pattern ist deterministisch und race-free.

---

## Was das für andere Tauri/Electron-Macs heißt

Wenn deine App auf macOS:

- **Synthetische Keystrokes** schickt (`enigo`, `CGEventPost`, AppleScript `keystroke`) → brauchst du **Accessibility**.
- **Screen-Region** capturet (`screencapture`, ScreenCaptureKit) → brauchst du **Screen Recording**.
- **Andere Apps scriptet** (`tell application "Finder"`, `osascript`) → brauchst du **Automation** per Target-App.
- **System Events scriptet** (frontmost-app probe, "key code" remote-typing) → brauchst du **Automation → System Events** *zusätzlich* zu Finder etc.
- **Globale Hotkeys** registriert → keydowns sind außerhalb deiner Webview verloren. Plan dafür entweder Event-Forwarding oder verzichte auf Webview-Interaktion mit diesen Tasten.

Plus den absoluten Killer für jeden iterierenden Entwickler:

- **Code-Signatur muss stabil sein**, sonst sind alle TCC-Grants nach jedem `cargo build` futsch. Selbst-signiertes Cert + Designated Requirement ohne `cdhash` ist die Pflicht — `tauri build` ohne Signing-Setup ist für Dev-Loop unbrauchbar.

Ohne dieses Setup würdest du dich nach jeder Build-Iteration durch 3-4 System-Settings-Klicks und ein App-Relaunch quälen. **Das** war der echte Killer bei Inspector Rust — Permission-Reset war so schmerzhaft dass es jede Iteration verlangsamte.

---

## Stelle in der Codebase

- `scripts/install-macos.sh` — der Build-+-Install-+-Sign-Flow mit dem dedizierten Keychain. Header-Kommentar erklärt das Why.
- `core/rust-lib/src/hotkey.rs` — `popup_is_visible`, `register_expander`, `register_direct_slots`. Suche nach "popup_is_visible" für die TCC-freie Frontmost-Logik, nach "expander-hotkey-forwarded" für die Event-Forward-Lösung.
- `core/frontend/src/App.tsx` — `useTauriEvent("expander-hotkey-forwarded", ...)` ist der Pwgen-spezifische Forward-Handler.
- `macos/src-tauri/entitlements.plist` — die App fragt explizit *nicht* nach `com.apple.security.automation.apple-events`-Entitlement. TCC ist die Quelle der Wahrheit; das Entitlement würde nur was bei Sandbox-Apps bringen, was wir nicht sind.

Wenn du das Pattern für deine eigene App klauen willst: nimm die `install-macos.sh` als Template + den `popup_is_visible`-Helper. Die zwei zusammen lösen 80 % vom TCC-Schmerz.
