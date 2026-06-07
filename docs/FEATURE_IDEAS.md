# Inspector Rust — Feature Ideas & Roadmap

A brainstorm of features that would round out IR as a "Swiss-Army-knife" desktop
utility, prioritised by **value ÷ effort** and grouped by how well they fit the
existing architecture (search-bar command palette · clipboard-centric · preview
panel · global hotkeys). This is a menu to pick from, not a commitment.

> Legend — effort: 🟢 small (frontend-only or one module) · 🟡 medium (new module
> + IPC) · 🔴 large (new subsystem). All ideas assume the existing patterns:
> a command goes in `COMMANDS` + `dispatchCommand`, a converter extends the calc
> engine, a content action surfaces in `PreviewPanel`.

---

## Tier 1 — High value, small effort (do these first)

1. **Pinned / favourite clips** 🟢🟡
   A persistent flag on history rows (★) that exempts them from the 1 000-row
   prune and surfaces them first / under a `pin` filter. Natural extension of the
   `entries` table (add `pinned INTEGER`); the most-requested clipboard-manager
   feature IR is missing. Pairs with a `pin`/`pinned` search filter.

2. **Smart clipboard actions in the preview** 🟢
   Detect the content type of the selected clip and offer one-tap actions:
   URL → open / make QR · email → compose · phone → FaceTime/tel · street
   address → open in Maps · tracking number → carrier page · `#hex` → already
   have colour · ISBN → lookup. Pure frontend detection + the existing
   `tauri-plugin-opener`. Makes the preview feel "alive".

3. **QR code generator** 🟢
   `qr <text>` (or a preview action on any text clip) renders a QR in the preview
   — share a URL/Wi-Fi/text to your phone instantly. Pure frontend (`qrcode` npm
   or a tiny hand-rolled encoder); copy the PNG to clipboard on Enter.

4. **Inline converters (extend the calc engine)** 🟢
   The calc box already evaluates math; teach it units:
   - `100 usd to eur` / `5 km in mi` / `72f to c` (currency needs a cached rate)
   - `0xff in dec` / `255 in hex` / `0b1010 in dec` (number bases)
   - `1717000000 as date` (epoch ↔ ISO), `10:00 berlin in tokyo` (timezones)
   Each is a pure parser returning a calc-style row. Huge utility, no new windows.

5. **Web-search "bangs"** 🟢
   Generalise the translate URL-open pattern: `g <q>` (Google), `gh <q>`
   (GitHub), `yt`, `npm`, `crates`, `so` (StackOverflow), `w` (Wikipedia),
   `dict <word>`. Data-driven table like `TRANSLATE_LANGS`. One-liners, very
   sticky.

6. **Dev quick-tools** 🟢🟡
   `json` (pretty/minify + validate clipboard JSON, error on bad) · `jwt`
   (decode header/payload of a clipboard JWT) · `uuid [n]` (generate) ·
   `hash <text>` or clipboard → md5/sha1/sha256 · `slug <text>`. Mostly frontend;
   hashing can reuse Rust.

7. **Clipboard auto-clear + app exclusion (security)** 🟢🟡
   - Auto-wipe the clipboard N seconds after a sensitive copy (opt-in).
   - **Exclude-app list**: never capture from 1Password/KeePass/etc.
     (`clipboard_watcher` checks `frontmost_app`). A privacy must-have.

---

## Tier 2 — High value, medium effort

8. **LAN clipboard sync** 🟡🔴
   Push/pull clipboard to other machines on the LAN. **The user already runs
   `go-sling` on raspi5** — IR could speak the same protocol, so a clip copied on
   the Mac is pasteable on another box. Killer feature for a multi-machine setup.

9. **Window management commands** 🟡
   `snap left` / `snap right` / `maximize` / `center` the frontmost window
   (macOS Accessibility API — already have the TCC grant for the expander).
   Rectangle/Magnet-style tiling without another app.

10. **Image format conversion + palette** 🟡
    `png2jpg` / `topng` / `towebp` on the clipboard image or a Finder selection
    (reuse the `image` crate already in `image_ops`). Plus `palette` — extract the
    dominant colours of a clipboard image into copyable swatches.

11. **Emoji / kaomoji / symbol picker** 🟢🟡
    `emoji <name>` → fuzzy-match an emoji DB, Enter copies/pastes. `kao` for
    ¯\\\_(ツ)\_/¯ & friends, `symbol` for → ™ ° µ. Frontend data file like
    `pwgen-dict.ts`.

12. **Snippet enhancements** 🟡
    - Snippet **folders/tags** + a fuzzy snippet launcher (`s <query>`).
    - **Fill-in placeholders** with a mini form (`{{name}}` prompts before paste)
      — extends the existing `snippet_template` engine.
    - Per-snippet **rich/multi-step** (cursor + tab stops).

13. **GIF / screen recording** 🔴
    Region screen-record → optimised GIF/MP4 (CleanShot-style), reusing the
    region picker + preview/pin flow. `ffmpeg`-free GIF is hard; could shell out.

14. **Reminders / recurring alarms** 🟡
    Extend `timer`/`alarm` with named, **recurring** reminders (`remind 9:00
    daily "standup"`) persisted in a table + a small agenda view. Optional
    Calendar/Reminders.app handoff.

---

## Tier 3 — Larger bets

15. **Mini HTTP client** 🔴
    `req GET <url>` → run a request, show status/headers/body in the preview
    (pretty-JSON). A pocket Postman. Pairs with the `json` formatter.

16. **Cross-device clipboard history (cloud)** 🔴
    Beyond LAN: an optional end-to-end-encrypted sync (the field-level crypto
    already exists). Privacy-first, opt-in.

17. **Scriptable actions / plugin hooks** 🔴
    Let a clip or a command pipe through a user shell script (`| myscript`),
    enabling arbitrary transforms. Power-user escape hatch.

18. **Spotlight-grade file search** 🔴
    `f <query>` → fuzzy file search (mdfind/`fd`) with open/reveal actions,
    extending the app launcher into files.

---

## Cross-cutting polish (any time)

- **History filters/scopes**: by content-type chips (text/image/files/links),
  by app of origin, by date — fast triage of a big history.
- **Paste stack** (CopyQ-style): queue several clips, paste them in sequence.
- **Onboarding / command cheat-sheet** overlay (`?` shows the palette).
- **Per-command usage stats** to auto-rank the most-used commands first.
- **Sync settings/snippets** via the existing backup JSON to a file the user
  syncs themselves (Drive/Dropbox).

---

## Suggested next 3 (my pick)

If picking three to ship next, for maximum "Swiss-Army-knife" payoff at low risk:

1. **Pinned clips + history filters** (Tier 1.1 + polish) — completes the core
   clipboard-manager story.
2. **Inline unit/timezone/base converters + web bangs** (Tier 1.4 + 1.5) — turns
   the search bar into a do-everything bar, all pure frontend.
3. **QR generator + smart preview actions** (Tier 1.3 + 1.2) — makes the preview
   panel genuinely useful on every clip.

LAN clipboard sync (Tier 2.8) is the standout *bigger* bet because the `go-sling`
infrastructure is already in place.
