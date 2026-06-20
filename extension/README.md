# Inspector Rust — Timesheet Bridge (browser extension)

Reports the **active tab** (URL + title) to Inspector Rust's local timesheet over
a **loopback WebSocket** (`ws://127.0.0.1:<port>`). This is the only way the
timesheet can attribute browser time to a host/title — and it's the *only*
network the app ever uses.

**Privacy:** the desktop app is the clock; this extension only sends URL/title,
only to `127.0.0.1`, only when you've entered the token. Nothing leaves your
machine. A "send host only" option strips the path/query before sending.

## Install (unpacked / temporary)

The extension is unpacked (MV3); no build step.

**Chrome / Edge / Brave / Arc (Chromium):**
1. Open `chrome://extensions`, enable **Developer mode**.
2. **Load unpacked** → select this `extension/` folder.

**Firefox:**
1. Open `about:debugging#/runtime/this-firefox`.
2. **Load Temporary Add-on…** → select `extension/manifest.json`.
   (Temporary add-ons are removed on restart; package via `web-ext` for a
   permanent install. Firefox MV3 service-worker support requires a recent
   version.)

## Configure

1. In Inspector Rust: **Settings → Timesheet → Browser extension** — copy the
   **port** and **token** (or "Regenerate" to rotate the token).
2. In the extension's **Options** page: paste the port + token, choose
   host-only vs full-URL, **Save**.
3. Start tracking in the app (`track on`). While a browser is frontmost, its
   intervals are now enriched with the active tab's host/title, and switching
   tabs splits the interval.

## Protocol

After the WebSocket opens, the extension sends:

- `{"type":"hello","token":"<token>"}` — must match; otherwise the server closes.
- `{"type":"tab","url":"…","title":"…","ts":<ms>}` — on tab activate / nav
  complete / window focus.
- `{"type":"blur"}` — when no browser window is focused.

The server (`tracking::bridge`) binds `127.0.0.1` only, rejects non-loopback
peers, and never sends anything outbound.
