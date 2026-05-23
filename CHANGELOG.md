# Changelog

All notable changes to Inspector Rust are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.26.3] — 2026-05-23

### Changed — OCR no longer saves the source PNG to history by default

The OCR pipeline used to upsert **two** history entries on every run — the source screenshot AND the recognised text — which doubled-up the history list with images you can't usefully paste back into a text field. The default is now **only the text**; the source PNG is captured for the recognition step and then discarded.

Settings → **Capture → "Keep OCR source image in history"** toggles the old behaviour back on for users who want to re-OCR or keep the source visible. Defaults to `false`. Persisted under the settings key `ocr.save_source_image`.

The system clipboard still receives only the recognised text (unchanged from before).

### Fixed — `Shift+↑` / `Shift+↓` system volume change is now instant

The volume shortcut spawned `osascript` **twice** per press (read current, then set new), ~150 ms each, so a single press took ~300 ms before the system moved — and a rapid Shift+↓ chord stacked latencies.

`adjust_system_volume` now:

- **Combines read + clamp + set into one `osascript` invocation** (multiple `-e` flags, atomic AppleScript). Saves ~50 % of the per-call latency.
- **Spawns the script on a worker thread** so the IPC resolves immediately — the next Shift+↑ / Shift+↓ press isn't queued behind the previous one. macOS plays its own native volume-change feedback, so the caller doesn't need to wait for the result.

Net result: pressing Shift+↑ feels native instead of laggy.

### Why 0.26.3

UX default flip + performance fix + new toggle — backwards-compatible (the old OCR behaviour is opt-in). Patch-level → `0.x.y`.

## [0.26.2] — 2026-05-23

### Fixed — HTML clipboard preview no longer clashes with the app theme

The HTML preview rendered the clipboard's HTML in a sandboxed iframe with a hardcoded white background, and the pasted HTML carried the source page's own inline `style="…"` attributes — so copying from any styled webpage produced a glaring white box on top of Inspector Rust's dark UI, often with the page's own colours leaking through (black-on-black blocks, neon highlights, etc.).

The iframe now:

- has its container `bg-` set to the app's `--color-surface` instead of hardcoded `bg-white`,
- ships a base `<style>` in its `srcDoc` that pulls live theme colours from the parent's CSS custom properties (`--color-fg` / `--color-surface` / `--color-accent` / …) and applies them with `!important` to `body, body *`, so pasted-in inline colours don't fight the theme,
- declares `color-scheme: dark` so browser-default scrollbars / form widgets match,
- gives `<a>`, `<code>`/`<pre>`, `<blockquote>`, `<table>` and `<img>` sensible theme-aware defaults.

Only colour and background are overridden — layout (margins, padding, sizing, borders' radius) survives, so the preview keeps the source's structure while reading like the rest of the app.

### Why 0.26.2

Visual polish for the HTML preview — no new feature, no breaking change. Patch-level → `0.x.y`.

## [0.26.1] — 2026-05-23

### Changed — `opener` easter egg: ← / → cycle through openers

Walking through the top-100 list via extra keystrokes (the seed-hash re-roll) was awkward. The opener row now reacts to **`←`** and **`→`** to step to the previous / next opener while the opener row is the selected entry:

- First activation seeds the index deterministically via `pickOpenerIndex(query)` — re-typing `opener` lands on the same starting line, so the easter egg feels predictable.
- The current pick lives in component state, so cycling state is preserved across additional keystrokes (the trigger is `^opener\b`, so `opener foo bar` still keeps your cycled pick).
- The arrow handler only attaches while `combined[selected].kind === "opener"`, so once you arrow Down to a clipboard row, ← / → fall through to the search-bar input's normal cursor-movement.
- HUD copy updated: "type any key to re-roll" → "← / → cycles to the previous / next opener" (HistoryItem chip tooltip + PreviewPanel hint).

`lib/openers.ts` gains the `pickOpenerIndex(seed)` helper (kept `pickOpener` as a thin wrapper). +3 unit tests pinning the new helper. Frontend total: **330**.

### Why 0.26.1

UX refinement of the v0.26.0 easter egg — no new feature surface, backwards-compatible. Patch-level → `0.x.y`.

## [0.26.0] — 2026-05-23

### Added — `opener` hidden German pickup-line easter egg

A third hidden trigger, alongside `getshaky` (Pong) and `rockthebox`/`rockthabox` (Snake). Typing **`opener`** in the popup search bar surfaces a random German pickup-line at the top of the list. Press Enter to paste it into the focused app.

- **Curated source** — 100 openers exported from the maintainer's `nicetobenice_db` PostgreSQL DB on the VPS (`69.62.121.168`), ranked by their personal ratings + favourites (DESC), tie-broken on the global `avg_rating`. Embedded as `core/frontend/src/lib/openers-data.ts` (no live DB call at runtime).
- **Re-roll on every keystroke** — the picker is a pure FNV-1a-style hash of the full query string. Identical query → identical pick (React render loop is stable, no flicker), and each extra keystroke (`opener `, `opener a`, `opener xy`, …) re-seeds → new pick.
- **Trigger** — `^opener\b` (case-insensitive, whitespace-tolerant): matches `opener`, `Opener`, `opener foo`, but NOT `openers` / `bopener`. Deliberately **not** in the `COMMANDS` catalogue → never appears in autocomplete; you have to know the word.
- **Integration** — new `kind: "opener"` in the `ListEntry` union; `HistoryItem` renders it with a `Sparkles` icon + an italic line; `PreviewPanel` shows the full text with a "type any key to re-roll" hint. Enter triggers `pasteText(opener)`.
- **Coverage** — 18 new tests (10 openers + 8 trigger), 327 frontend tests total.

### Why 0.26.0

A whole new interactive surface — backwards-compatible, no breaking changes. Feature-level → `0.x.0`.

## [0.25.2] — 2026-05-23

### Fixed — Direct-hotkey snippets now delete the typed abbreviation

If you typed an abbreviation (e.g. `aiplan`) and pressed the direct hotkey for that snippet, the body was *appended* — you got `aiplan<body>` instead of `<body>`. `expander::paste_snippet_body` now synthesizes `len(abbreviation)` Backspaces before pasting the body, so typed-then-trigger replaces the abbreviation cleanly (character count, not byte length, so multibyte abbreviations like umlauts work).

Trade-off, documented honestly: this is **blind** — the slot still doesn't read the field (otherwise it'd lose the "works in terminals" guarantee). Pressing the hotkey **without** first typing the abbreviation deletes N characters before the cursor. The normal flow is type-then-trigger, so this matches user expectation in the common case.

### Fixed — `Ctrl+Shift+S` now saves the screenshot on a single press

Before, the shortcut needed an awkward *double-tap within 1.5 s* to actually save a PNG to disk — a single press only wrote the image to the clipboard, and the only way to discover the "save" behaviour was to read the source. Now **one press** of `Ctrl+Shift+S`:

- writes the PNG to the system clipboard (as before),
- **auto-saves** to `~/Downloads/inspector-rust-screenshot-<timestamp>.png`,
- emits the existing `screenshot-saved` event so the frontend toast confirms the file path,
- and persists the history entry.

The double-tap mechanism is removed entirely (along with the `SCREENSHOT_SAVE_FILE` / `SCREENSHOT_LAST_MS` atomics and the Windows in-marquee `S`-key save-mode toggle); the only remaining state is `SCREENSHOT_IN_PROGRESS`, which still debounces a second press while the picker is open.

A file-write failure is non-fatal — clipboard and history still succeed, so the user never loses a capture.

### Added — Frontend tests for the IPC contract + fuzzy-search hook

- **`lib/ipc.test.ts`** (25 tests) — pins the IPC wrapper contract: every wrapper in `ipc.ts` calls `invoke("<rust_command_name>", {…})`, and the two halves are wired only by an exact string + the snake_case argument keys Tauri's auto-conversion expects. A typo on either side silently breaks the call. These tests mock `@tauri-apps/api/core` and assert command name, argument shape, default values, return-value pass-through, and error propagation across the seven IPC namespaces (history, snippets, notes, settings, expander, permissions, lifecycle).
- **`hooks/useFuzzySearch.test.ts`** (8 tests) — empty / whitespace queries return the entry list unchanged, substring + fuzzy matches surface the right rows, the no-match case returns `[]`, the `useMemo` cache holds across re-renders with identical inputs, recomputes when the query changes, and an empty entry list doesn't crash.

Total frontend test count: **309** (was 276); Rust workspace: **227** (was 216 in v0.25.1).

### Why 0.25.2

Pure test additions, no behaviour changes. Patch-level → `0.x.y`.

## [0.25.1] — 2026-05-23

### Fixed — "Set up permissions" now resolves the stale-TCC-entry case

The most common stuck state — *"the System-Settings switch is on, but Inspector Rust still asks for permission"* — wasn't handled by the v0.24.2 "Set up permissions" button, which only opened the System Settings pane. That case is a stale TCC entry: the stored code-requirement is from a previous binary (e.g. the pre-v0.23.2 ad-hoc signature) and doesn't match the current cert-signed binary, so `AXIsProcessTrusted` returns false even though the switch looks on.

The button now **always resets the TCC entry first** via `tccutil reset` (no admin password required) and re-fires the macOS permission prompt. Click *Allow → Open System Settings*, flip the switch once, and this time it sticks against the *current* signature. The same flow handles fresh installs (the reset is a no-op there). The card explainer is updated to say so.

### Added — Release artifacts for every supported OS/arch

`.github/workflows/release.yml` now ships a full set of bundles:

- **Windows x86_64** — `.exe` + `.msi` (unchanged).
- **Linux x86_64** — `.deb` **and `.AppImage`** (the bundle target list in `linux/src-tauri/tauri.conf.json` gains `appimage`; the workflow installs `libfuse2` and uploads the AppImage).
- **macOS Apple Silicon AND Intel** — matrix job (`macos-14` aarch64, `macos-13` x86_64). Each runner builds natively for its own arch (no cross-compile snags with the arch-specific `ort`/ONNX prebuilt binaries) and uploads the corresponding `InspectorRust_<ver>_<arch>.dmg`.

### Added — Unit tests for the new Linux CLI dispatcher

`core/rust-lib/src/cli_dispatch.rs::parse_args` (which routes `inspector-rust --toggle-popup` / `--ocr` / `--screenshot` / `--pick-color` to the running instance under GNOME/Wayland) gains 11 unit tests covering every alias, the help flag, unknown flags, multi-flag tie-breaking, and prefix-overlap guards.

### Why 0.25.1

A fix for a long-tail permission UX bug + release-workflow expansion + new test coverage. No breaking changes. Patch-level → `0.x.y`.

## [0.25.0] — 2026-05-23

### Added — Linux (Ubuntu / Debian) support

Inspector Rust now runs natively on Linux, merged from the community Linux port (PR #4). A new `linux/` bundle shell joins `win/` and `macos/` — the same thin 2-line `main.rs` calling `inspector_rust_core::run(...)`; all logic stays shared in `core/`.

- **Build** — `pnpm dev:linux` / `pnpm build:linux` → a `.deb` + AppImage. `scripts/install-linux.sh` provisions the apt deps, Node and Rust toolchain. Full prerequisites + a per-feature support matrix in [`linux/README.md`](./linux/README.md).
- **Region capture** (OCR + screenshot) — Wayland uses `grim` + `slurp`; X11 uses `scrot -s`. A missing tool produces a descriptive error naming the `apt` package.
- **OCR** — the `tesseract` CLI (`apt install tesseract-ocr` + language packs, e.g. `tesseract-ocr-eng` / `-deu`). Offline, no extra Rust dependencies.
- **GNOME / Wayland shortcuts** — Tauri's global shortcuts often don't receive key events under Wayland. The new `cli_dispatch` module exposes CLI flags (`--toggle-popup`, `--ocr`, `--screenshot`, `--pick-color`) routed to the running instance via `tauri-plugin-single-instance`; the Linux-only `desktop_shortcuts` module auto-registers GNOME/Cinnamon `gsettings` custom keybindings on first start.
- **Non-fatal shortcut registration** — a global-shortcut registration failure now logs a warning instead of aborting startup; the tray menu and CLI flags remain usable.
- System commands (kill / reboot / shutdown / lock) and the encryption keyring gained Linux backends. Data path on Linux: `~/.local/share/InspectorRust/history.db`.
- **Not yet on Linux** — the in-app eyedropper and the in-place AX text expander; the clipboard-paste expander fallback is used instead.
- A `.github/workflows/release.yml` job and the `inspector-rust.code-workspace` file round out the port.

### Why 0.25.0

A whole new supported operating system — backwards-compatible, no breaking changes. Feature-level → `0.x.0`.

## [0.24.2] — 2026-05-23

### Changed — consolidated macOS permissions card with one-click guided setup

The two separate amber permission banners (Accessibility, Screen Recording) are replaced by a single **macOS permissions** card with a **Set up permissions** button.

- **One-click chained setup** — clicking *Set up permissions* opens the first still-missing System Settings pane; the moment that grant flips on (the panel polls live), the card automatically opens the *next* missing pane. So one click walks you through both grants.
- Each permission has a live status row — an amber ring while missing, a green check + "Enabled" once granted — plus its own *Open* button.
- Troubleshooting (reset stale grants, re-check, quit) is tucked into one collapsible section instead of being duplicated across two banners.

**Note on automation:** there is no "grant everything with one password" — macOS deliberately does not let any app grant Accessibility or Screen Recording; the toggle must come from the user in System Settings, password or not. The button removes every other piece of friction (finding the panes, the right order, the stale-grant dance) but the final switch is, by Apple's design, yours to flip. Combined with the v0.23.2 stable-signing fix, you only ever do this once.

### Why 0.24.2

A UX rework of existing permission handling — no new IPC, no new capability, backwards-compatible. Patch-level → `0.x.y`.

## [0.24.1] — 2026-05-23

### Added — `rockthabox` wrap-around Snake variant

The `rockthebox` easter egg now has two modes, picked by the trigger spelling:

- **`rockthebox`** — *walls* mode (classic): hitting a wall ends the game.
- **`rockthabox`** — *wrap* mode: the snake reappears on the opposite edge instead of dying. Only a self-collision ends a wrap-mode game.

`lib/snake.ts::step` gained an optional `wrap` parameter (modulo the head back into the field instead of returning `dead`). `commands.ts` replaces `isRockTheBoxTrigger` with `rockTheBoxMode`, returning `"classic" | "wrap" | null`. The Snake HUD shows a `walls` / `wrap` mode chip. Pure-logic coverage extended (`snake.test.ts`).

### Why 0.24.1

A gameplay variant of the v0.24.0 easter egg — no new surface, backwards-compatible. Patch-level → `0.x.y`.

## [0.24.0] — 2026-05-23

### Added — `rockthebox` hidden Snake easter egg

A second hidden game, alongside `getshaky` (Pong). Typing **`rockthebox`** (or the variant **`rockthabox`**) into the popup search bar full-screen-takes-over the app shell with a game of Snake.

- **Gameplay** — steer with the arrow keys or **WASD**, eat the glowing food to grow, a wall or your own tail ends the run. The tick speed ramps up as your score climbs (capped so it stays playable). Score + a session-best are shown in the HUD; `Space` rematches, `Esc` quits.
- **Intro animation** — a ~1.9 s "box-assembling" flourish: the whole overlay rocks gently side-to-side while a glowing outline draws itself clockwise around the box, the grid dots sweep in on a diagonal wave, the snake's segments pop into place one by one (head first, with a back-ease bounce), and the food drops in with an expanding ring. The "ROCK THE BOX" title drops in with the letters spaced wide and snaps them tight.
- **Frame-rate independent** — the game advances on a fixed-timestep wall-clock accumulator, so it runs at the same real speed on 60/120/144 Hz displays (same lesson as the v0.23.1 Pong fix).
- Pure, unit-tested game maths in the new `core/frontend/src/lib/snake.ts` (`step`, `spawnFood`, `tickInterval`, collision rules — 24 tests); the stateful `<canvas>` loop is `components/SnakeGame.tsx`. Like `getshaky`, the trigger is **deliberately not** in the `COMMANDS` catalogue — it never surfaces in autocomplete; you have to know the word.
- Entirely client-side: no backend, no IPC, no new Rust module.

### Why 0.24.0

A whole new interactive surface (a second game mode), backwards-compatible. Feature-level → `0.x.0`.

## [0.23.2] — 2026-05-22

### Fixed — macOS permissions no longer need re-granting on every rebuild

`scripts/install-macos.sh` now signs every build with a **stable self-signed code-signing certificate** instead of leaving it ad-hoc-signed.

- **Root cause** — macOS TCC keys an Accessibility / Screen Recording grant to the app's code signature. An ad-hoc signature is keyed to the `cdhash` (binary hash), which changes on every rebuild → the grant was lost on every new version.
- **Fix** — the script creates (once, fully non-interactively) a self-signed certificate in a dedicated keychain `~/Library/Keychains/inspector-rust-signing.keychain-db` and signs with it. With a real certificate, TCC keys the grant to the app's *Designated Requirement* (`identifier "io.celox.inspector-rust" and certificate leaf = H"…"`) — which is **cdhash-free** and stable across rebuilds. Grant Accessibility + Screen Recording **once**; it now survives every future build.
- **One-time re-grant** — the first install after this change needs a single re-grant (the stale ad-hoc TCC entry won't match the new signature). The in-app Settings panel auto-detects the grant and offers the one-click relaunch as before.
- No admin password and no GUI prompt: the signing keychain has a hard-coded local password (it holds only a worthless self-signed key). If certificate creation fails for any reason, the script falls back to ad-hoc signing — it never hard-fails.
- The Settings panel's "Why does this keep happening on rebuild?" explainer is updated to reflect the new stable-signing behaviour.

### Why 0.23.2

Build-tooling fix for a long-standing macOS annoyance plus a docs-copy update — no runtime code change, no new IPC, backwards-compatible. Patch-level → `0.x.y`.

## [0.23.1] — 2026-05-22

### Fixed — `getshaky` Pong: frame-rate, serve delay, Shift boost, collision

Four fixes to the hidden Pong easter egg, all client-side (`lib/pong.ts` + `components/PongGame.tsx`):

- **Frame-rate independence** — the game ran "deutlich schneller" on a 144 Hz Windows display than on a 60 Hz MacBook because every frame advanced by a fixed step. The loop now scales all movement by `frameScale(dt)` — the wall-clock time since the previous frame, normalised to a 60 fps baseline — so the ball, both paddles and the Shift boost run at the same real-world speed on 60/120/144 Hz screens. A long stall (backgrounded tab) is clamped to 2.5× so the ball can't teleport.
- **1 s serve delay** — after a point the ball is parked at centre and the next serve fires `SERVE_DELAY_MS` (1000 ms) later, giving the player a beat to reposition.
- **Shift speeds up the paddle** — holding Shift while driving the paddle with the keys multiplies its travel speed by `SHIFT_SPEED_MULTIPLIER` (2×).
- **Swept paddle collision** — the per-frame point test is replaced by `paddleHit()`, a crossing test on the ball's leading edge: it registers a hit whenever the edge crossed the paddle face this frame, so a fast ball can no longer tunnel clean through a thin paddle.

New pure helpers `frameScale` / `paddleHit` + constants `REFERENCE_FRAME_MS` / `SHIFT_SPEED_MULTIPLIER` / `SERVE_DELAY_MS`, all vitest-covered (38 `pong.test.ts` tests).

### Why 0.23.1

Bug fixes to an existing feature, no new IPC, backwards-compatible. Patch-level → `0.x.y`.

## [0.23.0] — 2026-05-22

### Added — string-manipulation transforms on text entries

Select a **text** entry in the History list and the preview pane now shows a **Transform** toolbar — 11 string operations, each producing a new History entry + clipboard write (the original entry is untouched).

- **Transforms**: remove vowels, UPPERCASE, lowercase, Title Case, camelCase, snake_case, kebab-case, Base64 encode, URL encode (these nine are also keyboard-bound), plus Base64 decode and URL decode (click-only).
- **Keyboard**: `Cmd+1…9` on macOS / `Ctrl+1…9` on Windows trigger the first nine — the same `CmdOrCtrl` pattern as the existing `⌘B` / `⌘S` image actions. Plain digit keys can't be used (they'd type into the search bar); Shift+digit / Alt+digit type characters and Alt+1–3 collides with the text-expander hotkey, so `Cmd/Ctrl+digit` is the only conflict-free cross-platform choice.
- **Output**: each transform commits via the new `commit_transformed_text` IPC — clipboard self-write + a new Text History entry. Non-destructive; chain by selecting the new entry and transforming again.
- camel/snake/kebab share a tokeniser that breaks camelCase boundaries *and* splits on whitespace / `_` / `-`, so any of the three round-trips into any other. Base64 is Unicode-safe (`TextEncoder`/`TextDecoder`, not raw `btoa`). Decode transforms are total — invalid input is a no-op, never an error.
- Transform logic lives in the new pure, vitest-tested `core/frontend/src/lib/text-transform.ts` (24 tests); the `TransformBar` UI + `Cmd/Ctrl+1–9` handler are in `PreviewPanel.tsx`. Text entries only — image / files / html / rtf entries show no toolbar.

### Added — `mute` system command

The search-bar command palette gains **`mute`** — toggles the macOS system output mute (reads the current state via `osascript`, flips it). Like `lock` / `reboot` it surfaces in autocomplete. macOS-only; Windows returns "not implemented". IPC: `toggle_mute`.

### Why 0.23.0

A new interactive surface (the transform toolbar + `Cmd/Ctrl+digit` shortcuts), two new IPC commands, a new command-palette entry. Backwards-compatible. Feature-level → `0.x.0`.

## [0.22.0] — 2026-05-22

### Added — `Shift+↑` / `Shift+↓` adjust system volume

While the popup is open, **`Shift+ArrowUp`** raises and **`Shift+ArrowDown`** lowers the macOS output volume by 6 percentage points per press (≈ the 1/16 step macOS's own hardware volume keys use). Plain `↑`/`↓` still navigate the list — only the Shift modifier reroutes to volume.

- Backend: `system_commands::adjust_system_volume(delta)` reads the current level via `osascript`, applies the delta clamped to 0–100, sets it, and returns the new level. New IPC command `adjust_volume`. macOS-only — Windows returns "not implemented". The pure `clamp_volume` helper is unit-tested.
- Frontend: `useKeyboardNav` gained an `onShiftArrow` callback — `Shift+Arrow` invokes it (and skips list navigation) instead of moving the selection. App.tsx wires it to `adjustVolume(±6)`. Fire-and-forget; macOS plays its own volume feedback.
- No on-screen HUD — macOS's volume-change feedback sound is the confirmation, same as its hardware keys.

### Why 0.22.0

A new user-facing keybinding + a new IPC command. Compatible addition — plain arrow navigation is unchanged — but a new capability, so `0.x.0` per `docs/RELEASING.md`.

## [0.21.0] — 2026-05-22

### Added — `getshaky` 🏓 (hidden Pong easter egg)

Type **`getshaky`** into the search bar and the popup overlay shakes itself apart and reassembles as a game of Pong.

- **Hidden** — `getshaky` is *not* in the command catalogue, so it never appears in the autocomplete suggestions. It triggers only on an exact, fully-typed match (case-insensitive, whitespace-tolerant). You have to know the word.
- **The transformation** — a ~1.3 s flourish: the overlay jitters with an intensifying-then-settling shake (the "shaky" the command is named for), a big "GET SHAKY" title zooms in with an overshoot, then the play field + HUD fade in and the ball serves.
- **The game** — Pong against a bot, first to 5. Player paddle is driven by **mouse *and* arrow keys / W-S, both live at once**. The bot uses **ramp-up difficulty**: it starts fair and beatable (tracking-speed cap 4.5) and gains a little with every point it scores (cap → 7.5 at 4 points), so a deficit genuinely tightens. The ball speeds up slightly on every rally hit. Themed to the current Light/Dark palette — player paddle is the accent colour, board matches the app.
- **Esc is the only abort**, as specified. (After a match ends, Space offers a rematch — not an abort, so it doesn't break that rule.)
- Entirely client-side — a `<canvas>` + `requestAnimationFrame` loop. No backend, no IPC. Pure game maths (`clamp`, `botMaxSpeed` ramp-up, `paddleBounce` deflection, `serveBall`) lives in the new testable `lib/pong.ts`; the stateful loop + intro/over phases live in `components/PongGame.tsx`. `useKeyboardNav` gained an `enabled` flag so the popup's normal nav handler cleanly hands all keyboard control to the game.

### Why 0.21.0

A whole new (if playful) interactive surface — new module, new component, a search-bar trigger. No existing behaviour changed. Feature-level → 0.x.0.

## [0.20.2] — 2026-05-22

### Fixed — footer credit overflowing onto a second line

The footer is a fixed-height (`h-8`) single row: six keyboard hints on the left (`⏎ Paste`, `↑↓ Navigate`, `Esc Close`, `⌃⇧O OCR`, `⌃⇧S Shot`, `⌃⇧C Color`) and the credit + version + counter on the right. Six hints (OCR / Shot / Color were added incrementally over v0.9–v0.17) plus the verbose "made with ♥ by Martin Pfeffer" credit no longer fit the 600 px popup — the flex row wrapped, and the wrapped lines spilled out the bottom of the `h-8` strip.

Two-part fix:

- **Shortened the credit** — "made with ♥ by Martin Pfeffer" → "♥ Martin Pfeffer". The full wording is preserved in the hover `title` tooltip and the About dialog.
- **Widened the popup** 600 → 700 px. The list/preview split (40/60) and the cursor-monitor centring logic both scale automatically — no other change needed.
- Defensive: footer item groups are now `shrink-0` + `whitespace-nowrap`, so any future overflow clips cleanly at the edge instead of wrapping and breaking the row height.

### Why 0.20.2

Pure layout fix — a shorter string + a 100 px window-width bump + two CSS classes. Patch level.

## [0.20.1] — 2026-05-21

### Fixed — permission banners overlapping the Settings content (for real this time)

The two macOS TCC permission banners (Accessibility + Screen Recording) were `position: sticky`. The v0.16.2 attempt to fix their overlap gave them *staggered* `top` values so they'd stack instead of collide — but that just moved the bug: with both banners pinned at different heights, any section rendered between/below them (the new v0.20.0 **Appearance / Theme** section was the visible victim) got sandwiched and clipped between the two pinned bars.

Root cause: two **independently**-sticky elements in the same scroll container fundamentally don't coexist — there's no `top` arithmetic that makes scrolling content flow cleanly past *both*.

**Fix:** drop `sticky` from both banners entirely. They're now plain in-flow elements at the top of the Settings panel — the amber border + warning triangle keep them impossible to miss, and they scroll away like any other content when the user scrolls down. No pinning, no sandwich, no overlap.

### Fixed — stale `--color-text` in the permission banners

Two banner containers still used `text-[var(--color-text)]` — the CSS variable renamed to `--color-fg` in v0.20.0. The banner body text was resolving to an undefined variable. Corrected to `--color-fg`.

### Why 0.20.1

Two CSS/layout fixes in `SettingsPanel.tsx`, no API change. Patch level.

## [0.20.0] — 2026-05-21

### Added — Appearance theme control (Light / Dark / System)

Inspector Rust always *had* a dark theme — the `@theme` block in `styles.css` was the dark palette, and a `prefers-color-scheme: light` media query flipped to a light palette when the OS was in light mode. But that was invisible and un-overridable: the app simply followed macOS, with no way to force one or the other.

v0.20.0 makes the theme a first-class, user-controllable setting.

- **New "Appearance" section in Settings** — a three-way segmented control: **System** (follow the OS, the previous behaviour), **Light**, **Dark**. Light and Dark are hard overrides — they ignore the OS setting until you switch back to System. The choice persists in the `settings` table under `appearance.theme` and is re-applied on every launch.
- **Theme resolution** is now driven by a `data-theme` attribute on `<html>` (written by the new `lib/theme.ts`). `styles.css` carries explicit `:root[data-theme="light"]` / `:root[data-theme="dark"]` override blocks plus a system-scoped media query — so an explicit choice always wins, and "System" still tracks the OS live.
- **The dark palette was refined** — deeper near-black background (`#0c0d11`) with a faint cool undertone, the surface layer lifted enough to read as distinct, borders subtle but visible. Restrained, no neon. The light palette got a matching touch-up.

### Fixed — undefined `--color-fg` CSS variable

Components across the app referenced `var(--color-fg)` in hover states (`HistoryItem`, `AboutModal`, `SettingsPanel`, …), but `styles.css` only ever defined `--color-text`. `--color-fg` resolved to nothing, so those hover states silently fell back to inherited colour. Renamed the canonical variable to `--color-fg` (the name the component layer already standardised on) and defined it in every theme block — the hover states now work.

### Backend

- New IPC commands `get_theme_preference` / `set_theme_preference` (settings key `appearance.theme`), with a `normalise_theme` whitelist that collapses any unrecognised value to `"system"` so a hand-edited DB can't wedge the UI.

### Why 0.20.0

New Settings surface + two new IPC commands + a user-facing behaviour change (the app can now be themed independently of the OS). Compatible — a fresh install still defaults to `"system"`, i.e. the old behaviour. Feature-level → 0.x.0.

## [0.19.2] — 2026-05-21

### Added — Windows OCR + screenshot region parity, screenshot save-to-file mode

Merged via [#3](https://github.com/pepperonas/inspector-rust/pull/3). Brings the screen-region features — previously macOS-only — to Windows, and adds a save-to-file capture mode on both platforms.

- **Windows screen-region OCR** — `Ctrl+Shift+O` now works on Windows. Region selection uses a GDI fullscreen overlay; text recognition uses **WinRT `Windows.Media.Ocr`** + `Windows.Graphics.Imaging`. Picks up whatever OCR language packs are installed via *Settings → Time & Language → Language* — no bundled model, no extra install. COM is initialised per-thread on the capture worker; the WinRT futures are `.get()`-blocked to keep the pipeline synchronous like the macOS Vision path.
- **Windows screen-region screenshot** — `Ctrl+Shift+S` likewise works on Windows now (same GDI overlay, no OCR step).
- **Screenshot → save to file** — instead of writing the captured PNG to the clipboard, you can save it straight to disk via a native save dialog. On Windows the `S` key toggles the mode mid-overlay (the selection border turns green to confirm). On macOS — where `screencapture -i` is Apple's own process and can't have its keystrokes intercepted — a **double-tap of `Ctrl+Shift+S`** (second press within 400 ms of the first) flips the in-flight capture into save-to-file mode.
- **Docs** — README + README.de updated: Windows OCR/screenshot documented, the "macOS-only" limitation rows removed, a new note added about Windows OCR language packs. Region-picker module gained ~325 lines for the Windows path.

### Fixed — version manifests left at 0.19.1 by the merge

PR #3 bumped the README version badge to 0.19.2 but not the seven version manifests / `Cargo.lock` / the CHANGELOG. This release commit reconciles them — `Cargo.toml`, the four `package.json`s, both `tauri.conf.json`s, the three `Cargo.lock` workspace entries, and this CHANGELOG are now all 0.19.2.

## [0.19.1] — 2026-05-20

### Fixed — Color Picker on multi-screen setups (loupe appeared on main display instead of cursor display)

The `NSColorSampler` loupe always appeared on the **main display**, regardless of which monitor the user's cursor was actually on. Symptom: trigger `Ctrl+Shift+C` (or the in-modal Color Picker → "Pick from screen" button) with your cursor on a secondary monitor, and the magnifier appeared on the primary one — invisible to you until you moved the cursor over.

Root cause: macOS positions `NSColorSampler` on the calling app's **primary screen**. The "primary screen" is decided by where the app's most-recently-active window was. Inspector Rust's popup was hidden *before* the sampler was launched, and the popup's last known position (= whichever screen the user opened it on) was sometimes a different display than the cursor's. The `setActivationPolicy: Regular` + `activateIgnoringOtherApps:` pair that's needed to make `NSColorSampler` render its loupe then anchored the app to that stale screen.

**Fix:** before hiding the popup for either the eyedropper-pipeline (`Ctrl+Shift+C`) or the modal-flow Pick-from-screen button, park the popup at the centre of the cursor's monitor via the new `hotkey::park_on_cursor_monitor` helper (reuses the existing `pick_cursor_monitor` lookup that the popup-show path already uses). The hidden popup's "last seen" screen is then the right one, the activation snaps to the cursor's display, and the loupe renders where the user expects it.

- One-liner in two call-sites (`commands::run_eyedropper_pipeline` + `commands::pick_screen_color`); no behaviour change for single-screen users.
- No new dependencies. Cost: a single `set_position` call before each pick (~µs).

### Changed — fresh launcher icon set

App icons regenerated via `tauri icon` from `docs/inspector-rust.png` (the detective-themed hero artwork — same image used at the top of the README). Affects every bundled icon size: macOS `.icns`, Windows `.ico`, all `Square*Logo.png` Microsoft Store tile sizes, plus the platform PNG ladder (32×32 → 1024×1024).

- macOS Dock + Spotlight + Cmd-Tab → new icon.
- Windows Start menu + taskbar → new icon.
- New install ⇒ new icon. Existing macOS installs may need a Dock relaunch (`killall Dock`) to refresh the cached icon.

### Why 0.19.1

Two patch-level changes: a one-line multi-screen UX fix + an asset refresh (no code semantics changed by the icon swap). 0.x.y bump per `docs/RELEASING.md`.

## [0.19.0] — 2026-05-20

### Added — system-level power commands (kill / reboot / shutdown / lock)

Four new commands extend the v0.18.0 search-bar palette into a
proper power-user system control surface. Destructive commands
guard against accidents with native `window.confirm` dialogs;
locking the screen runs unconfirmed because it's cheap to undo.

**`kill [-9] [pattern]` — live process picker** *(macOS / Linux)*

Type `kill` alone → full process list (sorted by memory desc).
Type `kill slack` → filtered to processes whose name or exe path
contains "slack" (case-insensitive). Press Enter on a row → confirm
dialog showing PID + name + signal → SIGTERM is sent.

Add `-9` for SIGKILL: `kill -9 slack` filters the same way but
arms the row for force-quit instead of graceful shutdown. After a
successful kill the picker stays open and removes the killed PID
from the snapshot, so you can chain kills without re-typing.

- Backend: new `sysinfo`-crate-based `system_commands::list_running_processes` + `kill_process_by_pid(pid, force)`. List excludes the Inspector Rust process itself. ~10 ms for a full refresh on a typical desktop with 200+ processes.
- Frontend: new `ListEntry` kind `kill-target`; App.tsx detects kill-mode and overrides the whole list (history is hidden in kill mode — no point conflating clipboard rows with destructive process rows). New picker preview card in `PreviewPanel` with PID / memory / signal / executable path.

**`reboot` / `shutdown`** *(macOS only)*

Both shell out to `osascript` driving `loginwindow` via the legacy
Apple Events `aevtrrst` / `aevtrsdn`. No sudo required; macOS
handles its own "These apps have unsaved changes" dialog after
ours. Inspector Rust shows a native `window.confirm` first so a
typo-then-Enter doesn't reboot your machine.

**`lock`** *(macOS only)*

Shells out to `pmset displaysleepnow`. Instant, no confirmation —
the lock screen requires your password to dismiss, so the cost of
an accidental lock is just one password entry. No privilege needed.

### Why 0.19.0

Four new IPC commands + one new `ListEntry` kind + one new Rust
module + one new Cargo dep (`sysinfo`, ~150 KB). Backwards-compatible —
non-system queries route as before. New feature-level surface → 0.x.0.

### Windows

System commands are macOS-only in this release. Windows attempts return
`"not implemented on this platform"` and the frontend surfaces it as a
toast. Follow-up planned: `ExitWindowsEx` for reboot/shutdown,
`LockWorkStation` for lock, `OpenProcess` + `TerminateProcess` for kill.

## [0.18.0] — 2026-05-20

### Added — power-command palette in the search bar (six commands + autocomplete)

The search bar gains a shell-style command palette. Type a known
keyword + argument and Enter runs it; type a partial keyword and the
matching commands surface as autocomplete `hint` rows underneath.
Tab-completion not strictly needed — the suggestion row is itself
selectable, and activating it populates the search bar with the full
keyword prefix so you can just type the argument.

**Translation (open Google Translate in browser):**

- **`tren <text>`** — English → German.
- **`trde <text>`** — German → English.
- **`tr <text>`** — auto-detect → German.

Frontend constructs the canonical `https://translate.google.com/?sl=…&tl=…&text=…&op=translate` URL and opens it via `tauri-plugin-opener`'s external-URL handler. No translation runs locally; no network call from the app itself.

**Image ops (clipboard image in / out):**

- **`rz <W>x<H>`** — resize the clipboard image to the given dimensions via Lanczos3 sampling (best-quality downscaling), write the result back to the clipboard, push a fresh History entry. 16 MP target cap, `image` crate (already a workspace dep — no new system requirement).
- **`optim`** — read clipboard PNG, run through `oxipng` (lossless, zopfli + filter selection), save to `~/Downloads/inspector-rust-optim-<ts>.png`. Does *not* touch the clipboard. Returns before/after byte counts so the UI can confirm.

**Text:**

- **`rmvvls <text>`** — strip vowels (`aeiou` + uppercase + German umlauts `ä/ö/ü/Ä/Ö/Ü`) from text → clipboard + History entry. `rmvvls hello` → `hll`.

**Architecture:**

- New `image_ops.rs` Rust module (resize + optim pipelines, shared by IPC).
- Three new IPC commands: `resize_clipboard_image(W, H)`, `optimize_clipboard_image()`, `remove_vowels_to_clipboard(text)`.
- New workspace dep: `oxipng = "9"` (pure Rust, zero-config, statically linked, ~200 KB binary cost).
- New frontend `lib/commands.ts` with parser + autocomplete logic + `translateUrl` URL-builder.
- `ListEntry` discriminated union extended with `command` (runnable) and `command-suggestion` (autocomplete) kinds. Both render via existing `HistoryItem` + `PreviewPanel` paths.

**Tests** — 13 new Rust unit tests (`strip_vowels` + `image_ops` parse/serde) + 38 new frontend tests (`commands.test.ts` for parser/suggestions/URL builder/parseResizeArg).

### Why 0.18.0

Six new user-visible commands + new IPC surface + new frontend lib + new optional Cargo dep = clearly a feature release per `docs/RELEASING.md`'s 0.x.0 rule. Backwards-compatible — existing search behaviour unchanged when the input doesn't match a command keyword.

## [0.17.0] — 2026-05-20

### Added — `Ctrl+Shift+C` global eyedropper

- **New `Ctrl+Shift+C` global shortcut** fires the screen color picker directly from anywhere on the system. Cursor turns into the NSColorSampler loupe (macOS) / GDI overlay (Windows); one click on a pixel and the hex string (`#RRGGBB`) lands on the system clipboard **and** as a Text History entry. Parallel UX to the v0.15.0 `Ctrl+Shift+S` screenshot shortcut — fire-and-forget, no popup, no modal. The existing **Color Picker** button in the History tab still opens the HSV modal as before; this is the no-modal, just-give-me-the-hex path. — *#feat(color)*
- **Tray menu entry** "Pick Color (⌃⇧C)" / "Pick Color (Ctrl+Shift+C)" next to *Screenshot Region*. Same threading model as OCR + screenshot: dispatched to a worker thread.
- **Footer hint** gains `⌃⇧C Color` next to `⌃⇧O OCR` + `⌃⇧S Shot`.
- **Settings → Keyboard shortcuts** cheat sheet gains a row for the eyedropper alongside the OCR + screenshot rows.
- **Backend** (`commands.rs`): `run_eyedropper_pipeline(app)` reuses `screen_picker::pick_color_async` / `pick_color_blocking` but writes the hex to the clipboard via `ClipboardContext::set_text` + persists as a Text history entry instead of emitting `color-picked` for the modal. New `eyedropper_to_clipboard` IPC command (parallel to `screenshot_region`). New private helper `clear_eyedropper_no_popup` mirrors `clear_pick_suppress_hide` but doesn't re-show the popup window — appropriate for the global-hotkey flow.
- **Hotkey registration** (`hotkey.rs`): fourth global shortcut. `register_direct_slots` collision check now rejects `Ctrl+Shift+C` alongside popup / OCR / screenshot / expander.
- **No Screen Recording TCC grant needed** — NSColorSampler reads pixels via Quartz / GDI overlay reads via `GetPixel`, neither goes through `screencapture`.

### Why 0.17.0

New global shortcut + new IPC command + new tray entry + new event-emitting handler = feature-level addition per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule. Backwards-compatible — no existing functionality changed.

## [0.16.2] — 2026-05-20

### Fixed — overlapping permission banners in Settings tab

- **Both TCC permission banners (Accessibility + Screen Recording) had `position: sticky` with the same `top` value.** When one banner was expanded and the user scrolled, the other banner's *header* would stick on top of the first banner's *body* — the "Quit Inspector Rust / Force re-grant / Try system prompt" button block of the Accessibility banner would visually appear *below* the Screen Recording banner header, even though they belong to the Accessibility section. — *#fix(ui)*
- **Fix:** drop sticky positioning when a banner is expanded (the user is reading it, no need to pin it); stagger the `top` values when both banners are simultaneously collapsed-and-sticky so they stack instead of overlap.

### Why 0.16.2

Pure CSS / layout fix in `SettingsPanel.tsx`. No API change. Patch level.

## [0.16.1] — 2026-05-19

### Fixed — backup-export default filename regression from the v0.16.0 rebrand

- **Settings → Backup & restore → Export** proposed `inspector-rust-backup-.json` (no timestamp) instead of `inspector-rust-backup-<iso>.json`. The v0.16.0 brand rename ran a perl substitution that interpreted the JS template-literal `${stamp}` as a Perl variable lookup and silently dropped it. Caught during the v0.16.0 doc audit while sweeping for other rename damage; the file in question (`SettingsPanel.tsx`) is opaque to plain `grep` on this machine, which is why this and a dozen "ClipSnap" mentions slipped through the original rebrand. Now correctly proposes `inspector-rust-backup-2026-05-19T22-30-15.json` etc. — *#fix(backup)*

### Why 0.16.1

A one-line code fix to a user-visible default filename. Pure patch.

## [0.16.0] — 2026-05-19

### Changed — full rebrand: ClipSnap → Inspector Rust

This is a hard rebrand. Every user-visible "ClipSnap" string is now "Inspector Rust"; every technical identifier (Cargo package names, npm package names, bundle ID, app bundle, install paths) flipped to `inspector-rust` / `InspectorRust`. GitHub repo renamed from `pepperonas/clipsnap` to `pepperonas/inspector-rust`. **This is a breaking change at the install level** — see migration notes below.

- **Display name** (window title, tray tooltip, About modal, README, all docs): `ClipSnap` → `Inspector Rust` (two words, capitalised).
- **Bundle identifier**: `io.celox.clipsnap` → `io.celox.inspector-rust`. Triggers fresh macOS TCC grants on first launch (Accessibility, Screen Recording, PostEvent — all bound to bundle id + cdhash).
- **macOS app bundle**: `/Applications/ClipSnap.app` → `/Applications/InspectorRust.app`. **The old .app stays on disk** — uninstall it manually if you want a clean Spotlight / Launchpad. The new bundle name is CamelCase (no space) so terminal paths stay quote-free; the window title and tray label still render the spaced "Inspector Rust".
- **macOS LaunchAgent**: `~/Library/LaunchAgents/ClipSnap.plist` → `~/Library/LaunchAgents/InspectorRust.plist`. Old plist left in place — delete it manually or toggle autostart off in Inspector Rust before quitting the old build.
- **Data directory**: `~/Library/Application Support/ClipSnap/` → `.../InspectorRust/` (macOS); `%APPDATA%\ClipSnap\` → `%APPDATA%\InspectorRust\` (Windows). **Fresh start by design** — no auto-migration. To carry over snippets / notes / history, open the *old* ClipSnap one last time, Settings → Backup → Export, then import the JSON into Inspector Rust.
- **Keychain entry**: service `io.celox.clipsnap` → `io.celox.inspector-rust`. The old AES-256-GCM master key stays in Keychain (the migration plan above re-encrypts with the new key on import, so no plaintext leak).
- **Cargo packages**: `clipsnap-core` → `inspector-rust-core`, `clipsnap-win` → `inspector-rust-win`, `clipsnap-macos` → `inspector-rust-macos`. Lib code identifier `clipsnap_core` → `inspector_rust_core` (Rust auto-converts the hyphen).
- **Binary name**: `clipsnap` → `inspector-rust` (`win/src-tauri/Cargo.toml`'s `[[bin]] name`).
- **npm packages**: `clipsnap` → `inspector-rust`, `clipsnap-frontend` → `inspector-rust-frontend`, `clipsnap-{win,macos}` → `inspector-rust-{win,macos}`. The `pnpm dev:macos` / `pnpm build:win` aliases at the workspace root still work — they were already platform-named, not brand-named.
- **Release-artifact filenames**: `ClipSnap_<ver>_x64_en-US.msi` → `InspectorRust_<ver>_x64_en-US.msi`; `ClipSnap_<ver>_aarch64.dmg` → `InspectorRust_<ver>_aarch64.dmg`; the `clipsnap.exe` Windows standalone → `inspector-rust.exe`.
- **Output file prefixes**: `~/Downloads/clipsnap-image-<ts>.png` / `clipsnap-cutout-<ts>.png` → `inspector-rust-image-<ts>.png` / `inspector-rust-cutout-<ts>.png` (cutout-ML feature).
- **GitHub remote**: `https://github.com/pepperonas/clipsnap` → `https://github.com/pepperonas/inspector-rust`. GitHub auto-redirects the old URL for clones / git fetches, but please update your remotes (`git remote set-url origin https://github.com/pepperonas/inspector-rust.git`).
- **Win32 window class** (eyedropper overlay): `ClipSnapEyeDropper` → `InspectorRustEyeDropper`.

### Why 0.16.0

The rebrand changes the bundle identifier, the app bundle name, the data directory, and the binary name — anyone with the v0.15.x build installed will end up with both apps on disk after the upgrade. That's the upper bound of "breaking change" for a desktop app — `0.x.0` per `docs/RELEASING.md`'s SemVer policy.

### Migration notes

| You had                                           | After upgrade                                       | What to do                                              |
|---------------------------------------------------|-----------------------------------------------------|----------------------------------------------------------|
| `/Applications/ClipSnap.app`                      | `/Applications/InspectorRust.app` (new) + old one  | Manually drag the old `ClipSnap.app` to Trash            |
| TCC grants for `io.celox.clipsnap`                | Stale entries in System Settings → Privacy & Security | Manually remove them (or `tccutil reset ...`)             |
| Autostart entry (`~/Library/LaunchAgents/ClipSnap.plist`) | Old plist still firing on next reboot              | Delete it manually, or toggle autostart off in *old* ClipSnap, *then* delete the old app |
| Encrypted history at `~/Library/Application Support/ClipSnap/history.db` | Untouched on disk; unreachable from Inspector Rust | Open old ClipSnap → Backup → Export → import into Inspector Rust |

## [0.15.0] — 2026-05-19

### Added — dedicated screenshot region capture (no OCR required)

- **New `Ctrl+Shift+S` global shortcut** (literal Control on every OS, same convention as `Ctrl+Shift+O` / `Ctrl+Shift+V`): drag a marquee over any region → PNG lands on the system clipboard *and* in History. Same `screencapture -i` UX as `Cmd+Shift+4`, same Screen Recording (TCC ScreenCapture) gate as OCR, but **no OCR step** — works on regions that contain no recognisable text (a chart, a button, a UI mockup, a photo). The OCR shortcut still works as before; the screenshot shortcut is a strict superset of "what OCR couldn't preserve". — *#feat(screenshot)*
- **Tray menu entry** "Screenshot Region (⌃⇧S)" next to "OCR Region (⌃⇧S)". Same threading model — dispatched to a worker thread because `screencapture -i` blocks until the user finishes the marquee.
- **Footer hint** shows `⌃⇧S Shot` next to `⌃⇧O OCR` so the shortcut is discoverable every time the popup opens.
- **Settings → Keyboard shortcuts** cheat sheet gains a row for the screenshot shortcut alongside the OCR one.
- **Backend** (`core/rust-lib/src/commands.rs`): new `ScreenshotResult { cancelled, bytes }` type, `run_screenshot_pipeline(app)` function (parallel to `run_ocr_pipeline`), and `screenshot_region` IPC command. Shares `region_picker::capture` with OCR. Image is written to clipboard via `ClipboardContext::set_image` and persisted to history as a `[screenshot · N B]` entry. `mark_self_write(Image, b64)` arms the watcher so the round-trip doesn't double-record.
- **Hotkey registration** (`hotkey.rs`): added third global shortcut. `register_direct_slots` collision check now rejects `Ctrl+Shift+S` alongside the popup/OCR/expander hotkeys.

### Fixed — tray label for OCR shortcut

- macOS tray label said `OCR Region (⌘⇧O)` since the v0.14.1 hotkey change — the Cmd glyph should have been Control (`⌃⇧O`). Caught during the screenshot work; fixed in the same release.

### Why 0.15.0

New global shortcut + new IPC command + new event-emitting tray path = feature-level addition per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule. Backwards-compatible — no existing functionality changed.

## [0.14.2] — 2026-05-19

### Fixed — OCR history ordering: text on top, image below

- **OCR pipeline persists the source PNG *first*, then the recognised text.** Both rows get a `last_used_at` of `now()` at insert time, so the second insert wins the "most recent" slot. The popup sorts history `last_used_at DESC` — previously the *image* was on top (because text was inserted first), which is confusing because the *text* is the OCR result the user actually wanted: opening the popup post-OCR and pressing Enter pasted the screenshot instead of the recognised string. Now the text entry is on top and matches what's on the system clipboard. — *#fix(ocr)*
- No behaviour change for the clipboard write itself — `ctx.set_text` still runs once, before either history insert, with `mark_self_write(Text, ...)` so the watcher doesn't double-capture.

### Why 0.14.2

Pure ordering fix in `commands::run_ocr_pipeline`. No API surface change, no version-bump rationale beyond "patch level for a user-visible UX bug".

## [0.14.1] — 2026-05-19

### Changed — OCR hotkey is now literal `Ctrl+Shift+O` on every OS

- **macOS OCR shortcut moved from `⌘⇧O` to `⌃⇧O`** (literal Control, not Cmd). `Cmd+Shift+O` collides with **Go to Symbol** in VS Code, IntelliJ, WebStorm, and a host of other IDEs — pressing it inside an editor opened the IDE picker instead of triggering OCR. The Windows binding (`Ctrl+Shift+O`) was already correct; this just brings macOS in line. Same key combo, same physical position, no platform branching. — *#fix(macos)*
- **Hotkey registration** (`core/rust-lib/src/hotkey.rs`): both `register` and `register_direct_slots` now build the OCR `Shortcut` with `Modifiers::CONTROL | Modifiers::SHIFT` unconditionally — the `#[cfg(target_os = "macos")]` SUPER branch is gone. Direct-slot collision detection also tracks the new combo, so a slot can't shadow OCR.
- **Frontend display** (`core/frontend/src/components/Footer.tsx` + `SettingsPanel.tsx`): footer hint, Screen Recording explanation, direct-slot help text, and the Keyboard-shortcuts cheat sheet now render `⌃⇧O` on macOS (instead of `⌘⇧O`).
- **Docs** updated across `README.md`, `CLAUDE.md`, `macos/README.md`, and `docs/text-expander.md`. The Windows `Ctrl+Shift+O` references stayed correct.
- **Existing user impact** — pure muscle-memory change; the previous binding (`⌘⇧O` on mac) simply stops working after upgrade. Users who'd granted Screen Recording to Inspector Rust don't need to re-grant.

### Why 0.14.1

A targeted hotkey fix with no public-surface additions — pure patch.

## [0.14.0] — 2026-05-16

### Added — autostart UI: state-visible tray + Settings toggle

- **Tray menu's "Start at Login" / "Start with Windows" item is now a checkable menu item** that visibly reflects the current state (`☑` / ` `) and probes `~/Library/LaunchAgents/InspectorRust.plist` (macOS) / the run-key (Windows) on every tray build, so the checkmark stays right even if the autostart was enabled/disabled outside the app. Toggling updates the check in place and emits the new `autostart-changed` event so other UI surfaces stay in sync. — *#feat(tray)*
- **New "Startup" section in Settings** with a clearly-labelled "Start at login" (macOS) / "Start with Windows" toggle that explains where the entry lives — much more discoverable than the tray menu for users who don't routinely browse it. Listens for `autostart-changed` so toggling from the tray reflects immediately. — *#feat(ui)*
- **Two new IPC commands** `get_autostart_enabled` / `set_autostart_enabled` wrapping `tauri-plugin-autostart`'s `AutoLaunchManager`. Both read back the *now-effective* state from the OS rather than echoing the requested value, so the UI reconciles against actual filesystem / registry state if a toggle partially fails.
- The `tauri-plugin-autostart` default of `MacosLauncher::LaunchAgent` was already correct — no plugin-config change. Removed two dead-code lines (`let _ = autostart;` in setup; `let _ = MacosLauncher::LaunchAgent;` at the end of `build_tray`).

### Why 0.14.0

Adds a new event surface (`autostart-changed`), two new IPC commands, a new Settings section, and a tray menu item type change (`MenuItem` → `CheckMenuItem`). Compatible additions but a meaningful UX feature — new-feature bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.13.0] — 2026-05-13

### Added — direct hotkey → snippet slots (a paste-only expansion mode that works *everywhere*, including terminals)

- **New "Direct hotkey → snippet" section** in Settings → Text expander. Bind a hotkey straight to a snippet — e.g. `Alt+2` → the `aiplan` body — and pressing it pastes the body at the cursor, **no abbreviation typed**. Because it reads nothing from the focused field (it just writes the body to the clipboard and synthesizes `Cmd/Ctrl+V`, then restores the clipboard), it works in **any** app — including terminals (iTerm2, Terminal.app, kitty, Alacritty, …) where the abbreviation-based expander can't see the input line. — *#feat(expander)*
- **Backend**: `expander::DirectSlot { hotkey, snippet_id }` persisted as a JSON array under the `expander.direct_slots` settings key; `expander::paste_snippet_body` (AX-gated on macOS, same as the abbreviation expander); `hotkey::register_direct_slots` validates against collisions with the popup hotkey (`Ctrl+Shift+V`), the OCR hotkey, the abbreviation expander hotkey, and other slots, then registers each as a global shortcut whose handler dispatches to the main thread. Two new IPC commands `get_direct_slots` / `set_direct_slots`; `ExpanderShortcutState` grew a `direct` field; slots are re-registered from settings at startup. `snippets::get_by_id` added.
- **UI**: per-slot rows of `[hotkey recorder] → [snippet picker] [remove]`, an "Add slot" button, and a Save (which registers + persists; nothing is written if registration fails, so the previous slots stay live on error). A deleted bound snippet shows as `⚠ snippet deleted — pick another` so the slot can be rebound or removed. Missing-Accessibility warning mirrors the abbreviation expander's.
- **Why this mode exists:** the abbreviation expander ("type `aiplan`, press the hotkey") fundamentally can't work in a terminal — terminals don't expose the readline input buffer through accessibility, and a shell prompt has no GUI "select the word I just typed". Direct slots sidestep that by not needing to read anything.

### Why 0.13.0

New feature (a second expansion mode + its UI + storage + a new event-free IPC pair) with no breaking changes. New-feature bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.12.0] — 2026-05-12

### Fixed — text expander: hotkey now actually fires, failures are no longer silent

- **New default hotkey: `Alt+1`** (the `1`-row digit, not the numpad). The pre-0.12 default `Alt+Backquote` was *unreachable* on German ISO MacBooks — the physical `^`/`°` key under Esc reports as `IntlBackslash` (and on some layouts a different Carbon keycode), so the registered shortcut never matched the key the user pressed and the expander looked dead. Digit-row keys have a fixed `KeyboardEvent.code` on every layout, aren't dead keys anywhere, and aren't reserved by macOS or Windows. A one-time settings migration ([`expander::migrate_legacy_default`](./core/rust-lib/src/expander.rs)) bumps an un-customised `Alt+Backquote` install to `Alt+1`; a migration flag means it won't clobber a value the user deliberately re-picks afterwards. — *#fix(expander)*
- **Accessibility-missing no longer fails silently.** Previously, if macOS Accessibility wasn't granted, pressing the expander hotkey ran the whole capture/paste cycle — but `enigo`'s synthetic keystrokes silently no-op without the grant, so *nothing happened* and the user had no clue why. Now `expand_at_cursor` returns the `ax.permission_denied` sentinel instead of attempting a doomed clipboard roundtrip on macOS, and the hotkey handler pre-checks `AXIsProcessTrusted()` before dispatching — on a miss it pops the popup, switches to the Settings tab, and emits `expander-permission-needed` so the frontend shows an actionable amber banner ("Force re-grant → Restart now"). Mirrors the existing OCR `screen.permission_denied` pattern. — *#fix(macos)*
- **`diagnose_at_cursor` reports the real reason** when Accessibility is missing instead of an empty capture ("Accessibility permission isn't granted — … Grant it in the section above, then relaunch.").
- **Settings → Text expander: one-click presets** `Alt+1` / `Alt+2` / `Alt+3` next to the hotkey-capture button, so the common case doesn't require fighting the recorder widget. The capture widget still accepts any combination; help text now nudges toward digit keys for layout stability. Stored hotkey codes (`Alt+Digit1`) render in the friendly form (`Alt+1`) in tooltips, status text, and the keyboard cheat sheet.
- **Settle delay** (40 ms) at the start of the expand cycle so a physically-still-held `Alt` (from the hotkey itself) is released before `enigo` synthesizes its own modifier chords — avoids a stuck-modifier state in the source app. Invisible: the popup is hidden the whole time.
- **Expansion now works in Electron / Chromium / Mac-Catalyst text fields** (WhatsApp Desktop, Slack, Discord, VS Code, …). Those expose `AXValue` read-only: the old code set `AXSelectedTextRange` (which *selects* the abbreviation) then `AXSelectedText` (which returns success but does nothing), so the abbreviation just sat there highlighted, never replaced. The AX replace now **verifies** by re-reading `AXValue`; on a no-op it reports a new `ReplaceOutcome::SelectionActive` and `expander.rs` pastes the snippet body over the live selection (no re-select — `Cmd+Shift+←` would only swallow the previous word). Native Cocoa apps still get the clean in-place `AXSelectedText` replace with no clipboard touch. — *#fix(macos)*
- **Known limitation, now documented loudly:** the hotkey expander **cannot** work on a terminal command line (Terminal.app, iTerm2, kitty, Alacritty, WezTerm, …). Terminals don't expose the input line via AX, and there's no GUI-style "select previous word" shortcut on a shell prompt — pressing the hotkey there does nothing. Use the popup (`Ctrl+Shift+V` → search the abbreviation → Enter) for terminals.
- Windows is unaffected-positive: `Alt+1` registers cleanly there, `SendInput` needs no permission, and the UIA Backspace+type / clipboard-fallback paths are unchanged (the new `ReplaceOutcome` enum maps to `Replaced` / `Unsupported` there).

### Changed — bundled AI prompts: no more `[REQUIREMENT]` fill-in slots

- **All 25 `ai*` prompt snippets reworked** ([`core/rust-lib/src/seed/ai_prompts.json`](./core/rust-lib/src/seed/ai_prompts.json)) to drop the `[REQUIREMENT]` / `[CODE]` / `[CHANGE]` / `[SYSTEM]` / `[DOMAIN]` … input placeholders. The prompts are now the **structured-instruction half only** — designed to be appended to (or pasted alongside) your own prompt / code / context, so the subject comes from the surrounding text rather than a fill-in slot. Openers changed accordingly (`"…for: [REQUIREMENT]"` → `"…for the requirement at hand"`; `"the following code"` → `"the code at hand"`); choice-placeholders (`[PostgreSQL / SQLite / …]`, `[vitest / pytest / …]`, downtime budget, …) became `"as specified, or ask / default to X"` instead of literal brackets; the `## …` output structure is unchanged. — *#chore(snippets)*
- **Seed flag not bumped** (`seed.default_snippets_v1` stays). New installs get the new prompts automatically; existing installs keep their current `ai*` snippets until they click **Restore defaults** in the Snippets sidebar — deliberate, since a forced re-seed would clobber customised prompts and resurrect deleted ones.

### Why 0.12.0

Changes the default hotkey (a user-visible behaviour change with a settings migration), adds a new event surface (`expander-permission-needed`) and new public error sentinel, plus the presets UI. Beyond a 0.11.x patch — minor bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.11.0] — 2026-05-10

### Fixed — OCR no longer fails silently when Screen Recording is denied

- **Root cause.** macOS treats Accessibility and Screen Recording as **independent** TCC grants. Before this release, OCR pre-checks only knew about Accessibility — when the user had granted Accessibility (so paste worked) but never Screen Recording, pressing `⌘⇧O` would call `screencapture -i`, macOS would deny the spawn, the process would exit cleanly with an empty file, and the user saw … nothing. No marquee, no error, no clue. — *#fix(macos)*
- **New permission API** in [`core/rust-lib/src/screen_recording.rs`](./core/rust-lib/src/screen_recording.rs): `screen_recording_granted()` (`CGPreflightScreenCaptureAccess`), `request_screen_recording_grant()` (fires the macOS prompt), `open_screen_recording_settings()` (jumps straight to the right Privacy pane). Wired through four IPC commands plus a `tccutil reset ScreenCapture io.celox.inspector-rust` recovery path for stale grants.
- **`run_ocr_pipeline` pre-checks the grant** and returns the new `screen.permission_denied` sentinel when missing — same pattern as the existing `ax.permission_denied` for paste.
- **Hotkey handler surfaces the failure**: when `⌘⇧O` returns the sentinel, Inspector Rust now opens its popup and emits `ocr-permission-needed` so the frontend switches to the Settings tab and shows a clear amber banner pointing at the right System Settings pane. No more silent fail.
- **Settings panel** gets a second collapsible permission banner (parallel to the Accessibility one): one-line warning with `Open System Settings` button + chevron toggle for the full walkthrough (Quit · Force re-grant · Try system prompt · Re-check). Polls every second while not granted, like Accessibility, so the badge flips green within ~1 s of toggling in System Settings.
- **App-level toast banner** for the OCR-permission-needed event in `App.tsx`, mirroring the existing paste-failed banner. Auto-dismisses after 15 s (longer than the 8 s paste banner — the user needs more time to read + click into System Settings).

### Why 0.11.0

The change adds a whole new TCC permission grant the app depends on, plus four new IPC commands, a new Rust module, and a new event surface. That's beyond the bug-fix scope of a 0.10.x patch — minor bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.10.7] — 2026-05-10

### Added — Shortcut discovery

- **Footer now surfaces the OCR shortcut** (`⌘⇧O` on macOS, `Ctrl+⇧+O` elsewhere) next to the existing Paste / Navigate / Close hints. OCR was previously discoverable only via the tray menu, which most users rarely open. — *#feat(ui)*
- **New "Keyboard shortcuts" section in Settings** with a three-group cheat sheet: Global (Ctrl+Shift+V open popup, ⌘⇧O OCR, ⌥+` text expander), Popup list (Enter / Shift+Enter / arrows / Esc), and Image entry actions (⌘B cutout, ⌘S save). Modifier glyphs adapt to the running OS via the new `IS_MAC` helper in `core/frontend/src/lib/platform.ts`. — *#feat(ui)*
- The platform helper also exposes a `shortcut(...keys)` formatter so any future shortcut-rendering site can stay consistent without re-detecting macOS each time.

## [0.10.6] — 2026-05-09

### Changed — Accessibility banner is now collapsible

- **The Settings tab's Accessibility-required banner collapses to a single warning row by default.** When the macOS Accessibility permission is missing, the user sees a sticky amber-bordered bar with `⚠ Accessibility access required (macOS)` + the primary `Open System Settings` button + a chevron toggle. The full step-by-step walkthrough, the cdhash explanation, and the secondary buttons (Quit Inspector Rust / Force re-grant / Try system prompt / Re-check) only appear when the chevron is expanded. — *#chore(ui)*
- **Granted state is fully hidden** — when Accessibility is OK, no banner renders at all (previously the whole block was always present, which made the settings page feel cluttered for users who'd already granted). The `Restart now` prompt for the just-granted edge case still surfaces inside the Text-expander section as before.
- The collapsed bar stays prominent (amber border + warning icon + primary action button visible at all times), so the problem state is impossible to miss while occupying just one row of vertical real estate. — *#fix(ui)*

## [0.10.5] — 2026-05-09

### Fixed — Modals overflowing the popup window

- **About dialog** is now bounded to `max-h-[calc(100vh-2rem)]` and uses a three-row layout (sticky header / scrollable body / sticky footer). The natural height (~700 px) exceeded the 500-px-tall popup on the previous release, which clipped both the rounded top corners and the bottom credit line off-screen. The body now scrolls inside the modal, both sticky sections stay visible, the rounded `rounded-xl` corners are guaranteed visible. — *#fix(ui)*
- **Color picker dialog** gets the same `max-h-[calc(100vh-2rem)] overflow-y-auto` safety net so its rounded corners survive on small popup heights too. The picker is more compact (~450 px) so scrolling rarely triggers, but the constraint costs nothing and matches the About-dialog treatment.

## [0.10.4] — 2026-05-09

### Changed — UI consistency pass on modals

- **About dialog and Color picker dialog now share `rounded-xl` corners** (12 px instead of 8 px) for a softer, more macOS-native look. Inner cards inside the About dialog (identity block, workflow pitch) bumped to match. Establishes the visual hierarchy: modals = `rounded-xl`, inline cards/strips = `rounded-lg`, inputs/buttons = `rounded` / `rounded-md`. — *#chore(ui)*

### Added — Restore-defaults inline confirm

- **Snippets sidebar's "Restore defaults" icon now uses a two-step inline confirm**, matching the pattern History's "Clear all" introduced in v0.6.1. First click on the `RotateCcw` icon → toolbar row swaps to `Restore defaults? Yes / Cancel` in red; second click on `Yes` actually re-imports the bundled AI-prompt templates. Previously a single misclick would silently overwrite all default-abbreviation snippets — destructive without confirmation. — *#feat(snippets)*

## [0.10.3] — 2026-05-09

### Added — History time chip is now interactive

- **Hover the relative-time chip** (`just now`, `1h ago`, `3d ago`) on any history row → tooltip shows the absolute timestamps for both `Captured` and `Last used` (or `Captured: ... · (never re-used since)` when the entry hasn't been pasted again). — *#feat(history)*
- **Click the chip** → toggles the chip text in place between relative (`1h ago`) and absolute (`9 May 2026, 06:41:05`) display. `stopPropagation` so the click doesn't double-fire the row-select handler. Per-row state, so different rows can be in different display modes simultaneously.
- New `formatAbsolute(unixMs)` helper in [`core/frontend/src/lib/format.ts`](./core/frontend/src/lib/format.ts) using `Intl.DateTimeFormat` with the user's locale — matches Finder / Mail formatting muscle memory.

### Fixed — Snippets sidebar toolbar layout

- **Three sidebar actions are now icon-only.** `+ New Snippet`, `Import`, and `Restore defaults` previously wrapped two-line in the ~40 % sidebar column, with `Restore defaults` spilling outside the section. Replaced with three 28×28 icon buttons (`Plus`, `Upload`, `RotateCcw`) carrying the labels in `title` tooltips and `aria-label`s. — *#fix(snippets)*

## [0.10.2] — 2026-05-09

### Fixed — CI build on Linux runners

- **`ocr.rs` and `region_picker.rs` now have catch-all stubs for non-macOS / non-Windows targets.** Both modules were `#[cfg]`-gated for macOS + Windows but never declared a fallback impl, which made the `pub fn recognize` / `pub fn capture` wrappers fail to resolve their delegated `recognize_impl` / `capture_impl` symbol on Linux. The release CI runs on `ubuntu-latest` and broke as a result. The new stubs return `"OCR is not implemented on this platform"` / `"region capture is not implemented on this platform"`. — *#fix(ci)*
- Cleaned up the unused `anyhow::Context` import in `region_picker.rs` — only the macOS impl uses it, so it's now `#[cfg(target_os = "macos")] use anyhow::Context;`. Silences the `unused_imports` warning on Linux/Windows builds.

### Changed — README badge wall

- Doubled the badge set with grouped sections (Status / Platforms / Stack / Security / Quality / Community). Adds Linux planned, x86_64, ONNX Runtime, Apple Vision, U²-Net, AES-256-GCM, OS keychain, local-first, no-telemetry, offline, power-user, keyboard-first, Prettier, vitest count, contributors, forks, watchers, closed issues, PRs open, commit activity, lines-of-code. Test-count badge updated 98 → 107 (recolor + cutout + cutout_ml).

## [0.10.1] — 2026-05-09

### Added — Save image entry to Downloads

- **New "Save to Downloads" button + `Cmd/Ctrl+S` shortcut** below the cutout button on every image entry. Writes the selected entry's PNG bytes unchanged to `~/Downloads/inspector-rust-image-<ts>.png`. Companion to recolor — clicking a recolor swatch creates a new history entry with the tinted image; this lets the user grab that entry as a real file on disk without going through cutout (which would transform it). Same UX shape as the cutout button (busy state, saved-filename feedback, error toast). — *#feat(image)*
  - **IPC:** `save_image_entry_to_downloads(id) → path`. UI in `SaveImageButton` inside [`PreviewPanel.tsx`](./core/frontend/src/components/PreviewPanel.tsx).
  - Workflow: select image → recolor swatch → ↑ to the new tinted entry → `Cmd+S` → done.

## [0.10.0] — 2026-05-09

### Changed — Cutout switched from chroma-key to ML

- **U2Netp ONNX model now drives the cutout pipeline** (`cutout_ml.rs`). Cross-platform via the `ort` crate (ONNX Runtime, statically linked). Same architecture as Python's `rembg`, no Python dependency. — *#feat(cutout)*
  - **Why the switch.** The v0.8.0 chroma-key approach (corner-sampled background colour) only worked on truly uniform backgrounds. Real photos — airplane in gradient sky, person against cluttered background, anything where subject and background share colours — produced cutouts that left most of the background intact. Subject segmentation is the right tool; chroma-key is the wrong one.
  - **Pipeline:** decode any input format (PNG / JPEG / WebP / GIF / BMP) → resize to 320×320 → ImageNet-normalise → run U2Netp inference → resize the resulting saliency mask back to the original dimensions → apply as alpha on the original RGB → encode as PNG. ~1–4 s on CPU for a typical-size photo.
  - **Bundled artifacts:** [`core/rust-lib/models/u2netp.onnx`](./core/rust-lib/models/u2netp.onnx) (4.5 MB, Apache-2.0). The ONNX Runtime native library is statically linked via `ort`'s `download-binaries` feature, growing the release binary from ~12 MB to ~40 MB.
  - **Deps added:** `ort = "2.0.0-rc.12"` + `ndarray = "0.17"` (workspace); pulled into `core/rust-lib`. We tried `tract-onnx` first (pure Rust, no FFI) but it can't run U2Net's `Resize` ops with `pytorch_half_pixel` correctly; ort handles them natively.
  - **Old chroma-key code** in `cutout.rs` is kept around (marked `#![allow(dead_code)]`) as a future fast-path for known-uniform-background inputs.
  - **Tests:** 3 unit tests in `cutout_ml::tests` cover the smoke path (synthetic input → valid PNG out), oversize rejection, and corrupt-input rejection.

## [0.9.0] — 2026-05-09

### Added — Screen-region OCR (macOS)

- **`Cmd+Shift+O` triggers an interactive screen-region picker.** Drag a marquee over any text on screen, Inspector Rust runs Apple Vision OCR on the selection, writes the recognized text to the system clipboard, and pushes it into history. The source PNG is kept as a separate image entry so the user can re-OCR a different region without rescreenshotting. Tray menu also exposes an **OCR Region (⌘⇧O)** entry for discoverability. — *#feat(ocr)*
  - **Region picker** ([`region_picker.rs`](./core/rust-lib/src/region_picker.rs)) shells out to `/usr/sbin/screencapture -i -x -t png`, the same binary backing Cmd+Shift+4 — battle-tested marquee UX (Esc cancels, Space drags the rect, etc.) without reinventing an `objc2` overlay window. Captured PNG read from a temp file then deleted.
  - **OCR engine** ([`ocr.rs`](./core/rust-lib/src/ocr.rs)) uses Vision's `VNRecognizeTextRequest` (accuracy=Accurate, `usesLanguageCorrection=true`) via raw `objc2` `msg_send`. Joins one `\n` between observations (Vision returns one observation per visual line). Empty results are surfaced as `OcrResult { chars: 0 }` rather than an error so the UI can differentiate "engine ran but found nothing" from "engine failed".
  - **Build** — new `core/rust-lib/build.rs` emits `cargo:rustc-link-lib=framework=Vision` on macOS so the framework is linked. No new crate dependencies.
  - **IPC:** `ocr_region() -> { text, cancelled, chars }`. Both the global shortcut and the tray menu route through the shared `commands::run_ocr_pipeline(app)` helper, which dispatches the screencapture wait to a worker thread.
  - **Watcher integration:** the OCR pipeline calls `mark_self_write` before writing, so the clipboard watcher doesn't double-capture the result as a fresh user copy.
  - **Windows:** stubbed — both `region_picker::capture` and `ocr::recognize` return "not yet implemented on Windows" so the workspace still builds. Implementation will use `Windows.Media.Ocr` + a snipping overlay in a follow-up release.

## [0.8.0] — 2026-05-09

### Added — Image cutout / Freistellen

- **Background-removal action** in the image preview pane. Selecting an image entry shows a "Cut out background" button (plus `Cmd/Ctrl+B` shortcut); clicking it chroma-keys the image and saves the transparent PNG to `~/Downloads/inspector-rust-cutout-<timestamp>.png`. — *#feat(image)*
  - **Algorithm.** Sample the four corners of the image (8×8 patches per corner, median per channel — robust to subject pixels bleeding into the corner regions), treat that as the background colour, and replace each pixel with `alpha = 0` if its colour is within 30 RGB units of the background, `alpha = original` if beyond 50 units, with linear feathering in the band between (smooth cutout edge).
  - **Sweet spot.** Subjects on uniform backgrounds — sky, studio backdrops, solid logo fields. Cluttered / busy backgrounds hit the limit of chroma-keying; pro-grade results would need ML (rembg / U2Net), which is out of scope for a clipboard utility.
  - **Bounds & safety.** Hard cap at 16 megapixels. Output goes to `~/Downloads` (or `$HOME` if that doesn't resolve); the source history entry is left untouched.
  - **Module:** [`core/rust-lib/src/cutout.rs`](./core/rust-lib/src/cutout.rs) (~210 LOC). 5 unit tests cover background detection, subject preservation, oversize rejection, the all-background degenerate case, and transparent-corner handling.
  - **IPC:** `cut_out_image_entry(id) → saved_path`. Frontend wrapper in [`ipc.ts`](./core/frontend/src/lib/ipc.ts), UI in `CutoutButton` inside [`PreviewPanel.tsx`](./core/frontend/src/components/PreviewPanel.tsx).

### Added — About dialog + footer credit

- **About dialog** behind a button in **Settings → About**. Shows version, developer, license, year, target-audience pitch, and a tabular tech-stack overview (Tauri 2 / Wry / Rust / SQLite + AES-256-GCM / React 19 / TypeScript 5 / Vite 7 / Tailwind v4 / `image` 0.25). Esc / backdrop / X all close. — *#feat(ui)*
- **Author credit** ("made with ♥ by Martin Pfeffer") added to the popup footer next to the version chip and entry counter. — *#feat(ui)*

### Changed — Documentation

- **README rewrite.** Subtitle now reads "The keyboard-first clipboard toolkit for power users — Windows 11 & macOS"; new **Workflow** section frames the `Ctrl+Shift+V → type → Enter` loop; **Features** section reorganised by theme (Clipboard core / Text expander / AI prompts / Calculator / Color tools / Image tools / Notes / Backup / Plain-text paste / Tray + multi-monitor) with each block tightened to a scannable header + 3–6 bullets. Encryption (v0.6.0) promoted from "Limitations" into the Clipboard core feature list where it belongs.
- **Tauri bundle metadata** (`copyright`, `shortDescription`, `longDescription`) updated to drop the `celox.io` chatter and reflect the broader feature set / power-user audience. Bundle id stays `io.celox.inspector-rust` — that's a stable technical identifier the keychain & TCC depend on.
- **Snippet example signatures** anonymised to use `Your Name` / `https://example.com` placeholders so they're useful as templates for any user.

## [0.7.0] — 2026-05-08

### Added — Image recolor

- **Recolor toolbar in the image preview pane.** Selecting a mostly-grayscale image entry (logo, icon, silhouette) reveals a row of 9 preset swatches plus a hex input below the preview. Clicking a swatch or pressing Enter on a hex tints the image and stores the result as a new history entry — the original stays put. — *#feat(image)*
  - **Algorithm.** Decode PNG → for each RGBA pixel, replace RGB with `lerp(target, white, BT.601_luminance)`, preserve alpha → re-encode. Equivalent to ImageMagick's `+level-colors target,white`. Pure Rust via the `image` 0.25 crate (PNG-only feature set, no other format codecs pulled in).
  - **Photo guard.** Chromaticity sampling (`max((max-min)/max)` over up to 4096 opaque pixels) gates the UI: ≥ 0.12 hides the toolbar so saturated photos can't get accidentally tinted into Photoshop disasters.
  - **Bounds.** Hard cap at 16 megapixels to keep the synchronous recolor on the UI thread responsive on slower hardware.
  - **Module:** [`core/rust-lib/src/recolor.rs`](./core/rust-lib/src/recolor.rs) (~140 LOC). 6 unit tests cover dark→target mapping, white→white anchor, alpha preservation, oversize rejection, and chromaticity probe edges (pure-grayscale → ~0, pure-red → > 0.9).
  - **IPC:** `recolor_image_entry(id, hex) → new_id`, `image_chromaticity(id) → 0..1`. Frontend wrapper in [`core/frontend/src/lib/ipc.ts`](./core/frontend/src/lib/ipc.ts); UI in `RecolorToolbar` inside [`PreviewPanel.tsx`](./core/frontend/src/components/PreviewPanel.tsx).
  - **Deps added:** `image` 0.25 with `default-features = false, features = ["png"]` (avoids BMP/GIF/HDR/EXR/etc. baggage).

### Fixed — Clipboard capture priority

- **Image-before-files in the watcher.** macOS puts both the bitmap *and* the file path on the pasteboard when you copy an image file (PNG / JPG / HEIC) from Finder or use "Share → Copy Image" in many apps. The previous priority order (`files → image → …`) meant Inspector Rust stored only the path — users would see `/Users/.../foo.png` in history instead of the actual picture. Order is now `image → files → html → rtf → text`; pure file copies (PDFs, ZIPs, …) still capture as Files exactly as before. — *#fix(watcher)*

## [0.6.1] — 2026-05-07

### Fixed

- **Clear all confirmation** — replaced unreliable `window.confirm` (silent in Tauri's WebView2) with an inline "Delete N clips? Yes / Cancel" prompt in the history toolbar. — *#fix(ui)*
- **Bookmark visual feedback** — clicking the bookmark icon now shows a filled `BookmarkCheck` icon in accent color for 1.5 s so the user can see the note was saved. — *#fix(ui)*
- **Color picker modal height** — reduced SVPicker height (`h-44 → h-32`), swatch height (`h-16 → h-10`), and tightened margins so the modal fits inside the 500 px popup on Windows without scrolling. — *#fix(color-picker)*

## [0.6.0] — 2026-05-06

### Added — At-rest encryption for sensitive content

- **The SQLite database now encrypts every sensitive content field with AES-256-GCM.** Closes the long-standing "Unencrypted storage" limitation row in the README — passwords, tokens, snippet bodies, and note bodies are no longer readable to anyone who can `cat` the DB file. — *#feat(security)*
  - **Encrypted columns:** `entries.content_text`, `entries.content_data`, `snippets.body`, `notes.content_text`, `notes.content_data`. **Not encrypted:** timestamps, content-type tags, dedup `hash`, snippet abbreviations, note titles/categories — those are metadata that doesn't reveal clipboard content.
  - **Storage format.** Each encrypted value is stored as TEXT prefixed with `v1:` followed by base64 of `12-byte random nonce ‖ ciphertext+tag`. Legacy plaintext rows (no `v1:` prefix) are detected on read and returned as-is, then re-encrypted in place by the migration step at next startup. The migration is idempotent — already-encrypted rows are skipped.
  - **Key storage.** Per-install random 256-bit key kept in the **OS keychain** (macOS Keychain / Windows Credential Manager / Linux Secret Service) under service `io.celox.inspector-rust`, account `history-db-key-v1`. Falls back to a 0600 keyfile (`<data-dir>/.dbkey`) if the keychain is unavailable so the app stays usable instead of crashing. The fallback is strictly weaker — file-system access gets you the key — but matches the previous threat model floor.
  - **Roundtrip-safe across paths.** `save_from_clip` (Notes ← Clipboard) passes the already-encrypted ciphertext straight into the notes row instead of decrypt-then-reencrypt — same key, same scheme, ~free. `append_imported` from a JSON backup re-encrypts on the way in (backups stay plaintext for portability).
  - **Module:** [`core/rust-lib/src/crypto.rs`](./core/rust-lib/src/crypto.rs) (~280 LOC). 6 unit tests cover roundtrip, legacy plaintext passthrough, empty strings, fresh-nonce-per-encrypt, tampered-ciphertext rejection, wrong-key rejection.
  - **Deps added:** `aes-gcm` 0.10, `rand` 0.8, `keyring` 3 (cross-platform OS-keychain crate).

### Why 0.6.0

This is a feature with security implications and a one-time data migration on first launch — not a bug fix. Per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule, that earns a minor bump.

## [0.5.2] — 2026-05-06

### Added — System-wide screen color picker (eyedropper)

- **The Color picker modal now has a "Pick from screen" button** that lets you sample a color from anywhere on the desktop, not just inside Inspector Rust's own UI. The picked hex is automatically inserted into the modal — ready to copy as HEX / RGB / HSL. — *#feat(colors)*
  - **macOS:** uses Apple's own `NSColorSampler` (AppKit, 10.15+) — the same magnifier-loupe used by Pages, Keynote, and Sketch. Clicking outside the loupe cancels.
  - **Windows:** spawns a fullscreen layered overlay; click anywhere on screen to sample (`GetPixel` on the desktop DC). Press Esc to cancel.
  - **Async architecture.** The `pick_screen_color` IPC returns immediately; the result arrives later via the `color-picked` Tauri event with `string | null` payload. Keeps the UI responsive while the user is targeting their click.
  - New module `core/rust-lib/src/screen_picker.rs` (≈180 lines, fully `#[cfg(target_os = …)]`-gated). Adds `objc2` 0.6 + `block2` 0.6 as macOS-only deps for the Objective-C runtime calls; Windows reuses the existing `windows` 0.61 crate with extra features (`Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_UI_Input_KeyboardAndMouse`).
  - **Tahoe quirk worth knowing.** macOS Tahoe's `NSColorSampler` only renders its loupe when the calling app is a *Regular* (Dock-visible) NSApplication. Inspector Rust normally runs as `Accessory` (Dock-hidden tray app), so the picker briefly promotes the activation policy to Regular while the loupe is up, then demotes back 500 ms after the popup is restored. The popup itself stays visible during the pick — hiding it kills the loupe rendering ("no key window → no loupe").

### Docs

- README tagline updated to "Windows 11 & macOS"; previously said Windows 11 only.
- New / refreshed badges: separate Windows / macOS / Apple Silicon platform badges, plus Vite 7, ESLint flat-config, Vitest 3, cargo-test count, last-commit, repo-size, code-size, top-language.
- `docs/colors.md` rewritten end-to-end to describe the v0.5.x custom HSV modal, the click-to-select UX, and the screen eyedropper. The old "OS-native NSColorPanel / Win32 ChooseColor / GTK ColorChooser" copy was outdated since v0.5.0.

## [0.5.1] — 2026-05-06

### Fixed — Accessibility prompt fired on every paste

- **The actual root cause of "permission keeps re-prompting" is finally identified and fixed.** `enigo`'s `Settings::default()` ships with `open_prompt_to_get_permissions = true` on macOS — meaning every `Enigo::new()` call internally invokes `AXIsProcessTrustedWithOptions` *with the prompt option enabled*. So **every paste action on an untrusted process fired the standard "Inspector Rust would like to control this computer" dialog as a side effect** — even though we just wanted to silently fall back. — *#fix(macos)*
  - **Fix:** new `enigo_settings()` helper in `paste.rs`, `expander.rs`, and `text_field/windows.rs` constructs `Settings { open_prompt_to_get_permissions: false, ..Settings::default() }`. Every `Enigo::new()` now uses it. enigo silently returns `NoPermission` when the process is untrusted; the dialog never fires as a paste-time side effect.
  - **Plus AX guard at the top of every paste IPC.** `paste_entry`, `paste_entry_formatted`, `paste_text`, `paste_snippet`, `paste_note`, `paste_note_formatted` all start with `require_accessibility()?` — short-circuits before even touching enigo and returns the structured `ax.permission_denied` error string to the frontend.
  - **Frontend toast.** `App.tsx` catches paste errors and renders an amber sticky banner: *"Paste failed — macOS Accessibility access not granted. Open the Settings tab and click Force re-grant…"* with an **Open Settings** button. Auto-dismisses after 8 s. The user finally has clear feedback instead of a silent failure or a recurring system dialog.
- **Live-debug methodology** documented in the commit history (kept in `git log` rather than the codebase): a temporary background AX-poller revealed that `AXIsProcessTrusted()` does *not* cache per-process on Tahoe — it re-queries TCC on every call. So our SettingsPanel polling has always been correct; the `ax.permission_denied` toast is the right user-facing complement.

### Changed — Color picker UX

- **Modal opens in a "no selection yet" state.** v0.5.0 default-filled the picker with `#3366FF` so the toolbar-button click felt like it had already selected a color. Now the modal opens with: empty hex input, dashed-border placeholder swatch reading "Click in the picker above (or type a hex) to select a color", and Copy disabled. **The first click in the SV picker is the selection** — matching the user's mental model of "1st click opens, 2nd click selects". — *#fix(colors)*
  - SV-picker crosshair indicator hidden until first click.
  - Hue-slider drag and hex-input typing also count as "selection" once the user engages with them.
  - Closing & re-opening the modal resets to the no-selection state.

## [0.5.0] — 2026-05-05

### Added — 25 default AI prompt snippets, working color picker

- **Bundled default snippet library — 25 curated AI prompts.** First-launch seeds your snippet table with `ai*`-prefixed prompts covering programming (`aiplan`, `aireview`, `airefactor`, `airegex`, `aisql`, `aitest`, `aimigration`, `aibench`), web/frontend (`aithumb`, `aimobile`, `aia11y`, `aiseo`, `aicomponent`), IT security (`aithreat`, `aipentest`, `aiauth`, `aigdpr`), business workflows (`aibrief`, `airfp`, `aiokr`, `aichange`), data analysis (`aidataq`, `aiml`, `aidashboard`), and architecture (`aiapi`). Each prompt is a structured, opinionated brief — sections, bullets, output-format directives — written to be handed straight to an LLM without further massaging. Type the abbreviation in the search field, press Enter (or use the text expander), get the full prompt. — *#feat(snippets)*
  - **Idempotent seeding.** Tracked via `seed.default_snippets_v1` in the settings table. Runs once on first install; user-deleted prompts stay deleted on subsequent launches.
  - **Restore defaults button** in the Snippets-tab sidebar (rotate-counter-clockwise icon, next to Import). Re-imports all 25 prompts, upsert-by-abbreviation — your custom snippets with different abbreviations are untouched, but a deleted/edited `aiplan` *is* reset to the bundled version.
  - Embedded via `include_str!` so no external file is needed at runtime.
  - 3 new Rust unit tests (`embedded_json_parses_and_has_25_prompts`, `maybe_seed_inserts_on_first_run_and_skips_after`, `restore_defaults_re_imports_explicitly`).
- **Working cross-platform color picker.** v0.4.0's HTML5 `<input type="color">` was unreliable in WKWebView (Tauri's macOS renderer) — the OS picker often didn't open, and even when it did, `navigator.clipboard.writeText` got blocked because the `change` event fires outside the user-gesture context. Replaced with a **custom modal** that runs entirely in the WebView. — *#fix(colors)*
  - Hue slider + 2D saturation/value picker + live hex input + format tabs (HEX/RGB/HSL) + WCAG-readable preview swatch + Copy button.
  - Clipboard write goes through `@tauri-apps/plugin-clipboard-manager`'s `writeText` (no browser-API restrictions).
  - Esc / backdrop-click closes; copy feedback flashes "Copied!" for 2s.
  - Capabilities updated: `clipboard-manager:allow-write-text` added to both `macos/src-tauri/capabilities/default.json` and `win/src-tauri/capabilities/default.json`.

### Why 0.5.0 (not 0.4.3)

The 25-prompt seed is a real new feature surface, AND first-run behavior changes (new users automatically get a populated snippet library — that's an opinion, not a fix). Bumping minor signals it.

### Tests

`cargo test --workspace`: **84 → 87 green** (+3 seed). `pnpm test`: **77 → 85 green** (+8 HSV/HSL/hex helpers).

## [0.4.2] — 2026-05-05

### Fixed

- **No more duplicate history entries from plain-text paste.** v0.4.0's plain-text-paste downgrade for HTML / RTF clips was leaking back into the watcher: Inspector Rust wrote the plain-text version of an HTML clip to the OS clipboard → the clipboard watcher saw the change → recorded a *new* Text-type entry `just now`, sitting next to the original HTML clip from earlier. Hash-based dedup didn't catch it because `hash(Html, "<p>foo</p>") ≠ hash(Text, "foo")`. — *#fix(watcher)*
  - **Fix:** `WatcherState` gets a one-shot `self_written: Mutex<Option<String>>` fuse holding the SHA-256 of the most recent payload we wrote ourselves. The watcher checks this hash before storing and consumes-and-skips any matching event. Every paste IPC (`paste_entry`, `paste_entry_formatted`, `paste_text`, `paste_snippet`, `paste_note`, `paste_note_formatted`) calls `watcher.mark_self_write(content_type, payload)` immediately before triggering the OS clipboard write. Net effect: pasting from history never creates a duplicate entry, regardless of the plain-text setting.
- **Macros prompt no longer fires as an unwanted side effect.** When `expand_at_cursor` (hotkey trigger) or `diagnose_at_cursor` (Test button) call `AXUIElementCopyAttributeValue` on the system-wide element while Inspector Rust is **untrusted** (typical post-rebuild stale-cdhash state), macOS triggers the standard "would like to control this computer" prompt as a side effect — even when we just want to silently fall back to the clipboard path. — *#fix(macos)*
  - **Fix:** both functions now check `accessibility_granted()` *before* calling any AX function. When `false`, they go straight to the clipboard fallback (or return an empty diagnose result), and the macOS prompt isn't triggered as a no-op cost. The Settings panel's amber banner + **Force re-grant** button remain the right place to surface the underlying permission issue.

## [0.4.1] — 2026-05-05

### Changed

- **`paste_note` now respects `paste.plain_text_only`.** v0.4.0 added the plain-text-paste toggle for clipboard history, but notes (a separate paste path via `paste_note`) kept their old behaviour — HTML / RTF notes always pasted with formatting. The user's original ask was "always plain text in all OSes" which implicitly covers notes too. Now: HTML / RTF notes get downgraded to their plain-text preview when the toggle is on; image / files notes remain unaffected. — *#fix(paste)*
- New `paste_note_formatted` IPC command mirrors `paste_entry_formatted` — bypasses the setting and uses the note's original content type. Wires up symmetrically; the NotesPanel UI doesn't surface a Shift+click override yet but the IPC is ready when we add one.

### Docs

- `docs/notes.md` paste-behaviour table updated to call out which content types respect the plain-text-only toggle and which are unaffected.

## [0.4.0] — 2026-05-05

### Added — Plain-text paste, hex color preview, color picker

- **Plain-text paste mode (default on).** Settings → Paste section gets a new toggle. When on, HTML and RTF clipboard entries are stripped to their plain-text preview at paste time — so copy-from-Word / browser / mail and paste-into-anything no longer leaks the source app's font / colour / hyperlink styling. The original formatted content is preserved in the history (preview pane still renders it; the type icon still shows HTML / RTF), only the *paste action* downgrades. Image / Files entries are unaffected. — *#feat(paste)*
  - **Per-row override:** hold <kbd>Shift</kbd> while pressing <kbd>Enter</kbd> in the popup to paste *with* original formatting, regardless of the toggle. New IPC `paste_entry_formatted` bypasses the setting; `useKeyboardNav` forwards `event.shiftKey` to the activate handler.
  - Backend: `paste.plain_text_only` setting key (default `true`); `paste_entry` reads it and routes Html / Rtf entries to `paste::paste_text(content_text)`. `paste_entry_formatted` always uses `paste::paste_entry` for original-content-type behaviour.
- **Inline hex color preview** in the search input — Alfred-style. — *#feat(colors)*
  - Type `#3366FF` (or `3366FF`, `#abc`, `#abcdef12`, …) and a color row appears as the top list item with a swatch + hex + RGB summary. Press <kbd>Enter</kbd> to paste the canonical `#RRGGBB` (uppercase) into the previously focused app.
  - Heuristic: 3 / 4-digit forms require the `#` prefix (too ambiguous with search otherwise — `abc`, `f00d`, …); 6 / 8-digit forms accept either form.
  - Preview pane shows a full 128 px swatch with the hex overlaid (foreground auto-picked black/white via WCAG luminance for readability), plus copy-to-clipboard buttons for hex / `rgb(…)` / `hsl(…)` strings.
  - Pure frontend (`core/frontend/src/lib/colors.ts`); 24 vitest cases covering valid / invalid / canonicalisation / RGB-HSL conversion / readable-foreground.
- **OS-native color picker** — new "Color picker" button in the History tab's toolbar. Opens an `<input type="color">` which Tauri renders via the OS-native picker (NSColorPanel on macOS, Win32 ColorDialog on Windows, GTK ColorChooser on Linux). The chosen hex (uppercase) is written to the system clipboard via the Web Clipboard API; the watcher captures it as a fresh history entry within the next event tick. — *#feat(colors)*

### Changed

- `App.tsx` activate handler: signature changes to `activate(i, shiftKey)`. Color-row activation pastes the canonical hex via the existing `paste_text` command. Calc-row activation unchanged.
- `useKeyboardNav.onEnter` callback signature is now `(shiftKey: boolean) => void`.
- `HistoryItem` and `PreviewPanel` learn a fourth row kind (`color`) alongside clip / snippet / calc.
- `ListEntry` discriminated union gains `{ kind: "color"; data: ColorEntryView }`.

### Tests

`pnpm test`: **53 → 77 frontend** (+24 colors tests). `cargo test --workspace`: 84 unchanged (paste-plain-text logic exercises through existing paste tests; the wiring is straightforward enough that integration testing is overkill here).

### Why 0.4.0 (not 0.3.2)

Plain-text-paste-by-default is a **behaviour change**: clipboard entries that *used* to paste with formatting now arrive as plain text, by default, without the user opting in. That's a semver-meaningful flip. Two new user-facing features (hex preview, color picker) compound it. Bumping minor signals the change.

## [0.3.1] — 2026-04-29

### Fixed

- **macOS Accessibility prompt loop after rebuilds.** Common state after a real source-change install: the toggle in System Settings → Accessibility shows Inspector Rust as **enabled**, but Inspector Rust still asks for permission on every hotkey press. Cause: the toggle's underlying TCC entry is bound to the *previous* binary's cdhash; the new build has a different cdhash and is treated as a new app. The toggle UI just reports the bundle id, which masked the discrepancy.
  - **Fix:** new **Force re-grant (clear stale)** button in the amber Accessibility banner. Shells out to `tccutil reset Accessibility io.celox.inspector-rust` + `tccutil reset PostEvent io.celox.inspector-rust` (no sudo needed for the user's own bundle), then fires `AXIsProcessTrustedWithOptions(prompt: true)` so macOS re-adds Inspector Rust to the Accessibility list with the *current* cdhash. Toggling on again creates a TCC entry that matches what the running process actually is. — *#fix(macos)*
  - The legacy "Try system prompt" button stays as a secondary option (for the rare cases where the entry is sane and just needs a re-prompt).
- New IPC command `force_reset_and_request_grant` (macOS-only meaningful behaviour; no-op elsewhere). Backend in [`core/rust-lib/src/expander.rs`](./core/rust-lib/src/expander.rs); wrapper in [`core/frontend/src/lib/ipc.ts`](./core/frontend/src/lib/ipc.ts).

## [0.3.0] — 2026-04-28

### Added — Accessibility-first text expander

- **The text expander now reads the focused field directly via the OS accessibility layer** instead of synthesising `Cmd/Ctrl+Shift+←` + `Cmd/Ctrl+C` as the *primary* path. macOS uses **`AXUIElement`** (ApplicationServices), Windows uses **`IUIAutomation`** (UIAutomationCore). Same Accessibility permission already required for paste; no new permission added. Native FFI — no objc2/winRT macros needed. — *#feat(expander)*
  - **Why it matters:** the keystroke approach works in 90 % of apps but breaks in terminals (iTerm2, kitty, gnome-terminal — they reinterpret `Cmd/Ctrl+Shift+←` as pane-switch / mark-selection), web apps with custom keyboard handlers (Google Docs, online IDEs), and password fields. The accessibility approach succeeds wherever the focused element exposes its value to assistive tech — which is essentially every text field a screen reader can read.
  - **No more clipboard touch on the happy path.** When AX/UIA succeeds the user's clipboard is left completely untouched and there's no visible selection flicker.
  - **Clipboard fallback retained.** When the focused element doesn't expose the necessary attributes (rare native Carbon, Java/Swing without AccessBridge), Inspector Rust falls back to the previous keystroke + clipboard roundtrip seamlessly.
- **`text_field` module** — new abstraction in [`core/rust-lib/src/text_field/`](./core/rust-lib/src/text_field/):
  - `mod.rs` — `FieldAccess` trait + `CapturePath { Ax, Uia, Clipboard }` enum + UTF-16 ↔ char-index helpers + the platform-agnostic `word_start_before_cursor` algorithm. 7 unit tests covering ASCII, German umlauts, emoji (supplementary plane), cursor past end, whitespace-only.
  - `macos.rs` — raw FFI to `AXUIElementCreateSystemWide` / `AXUIElementCopyAttributeValue` / `AXUIElementSetAttributeValue` for the three attributes that matter: `AXFocusedUIElement`, `AXValue`, `AXSelectedTextRange`. UTF-16 helpers because AX reports cursor positions in UTF-16 code units. 3 unit tests.
  - `windows.rs` — `windows` crate bindings to `IUIAutomation`, `IUIAutomationTextPattern`, `IUIAutomationTextRange`. Uses UIA for the *read* (reliable) but deliberately uses Backspace×N + `enigo.text(body)` for the *write*, because UIA's `IUIAutomationTextEditPattern2::Replace` is patchily implemented across real-world Windows controls.
- **`Capture path` row in the Diagnose UI** — Settings → *Text expander* → Diagnose now shows whether the run used `macOS AX (clean — no clipboard touch)`, `Windows UIA (clean — no clipboard touch)`, or fell back to the `Clipboard fallback` path. Lets you tell at a glance whether the app you're testing in has working accessibility.

### Changed

- `expander::expand_at_cursor` and `expander::diagnose_at_cursor` now try AX/UIA first; the legacy clipboard roundtrip is the second-choice fallback. The fallback path can also be invoked with prefetched abbreviation+body so the lookup isn't repeated when AX read succeeded but AX replace didn't.
- `core/rust-lib/Cargo.toml` — added `windows = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Com", "Win32_UI_Accessibility"] }` as a `target.'cfg(target_os = "windows")'` dependency. macOS / Linux builds don't pull it in.
- **`DiagnoseResult`** gains a `path: "ax" | "uia" | "clipboard"` field. Frontend `ipc.ts` interface updated to match.

### Why bump to 0.3.0

This is a real architecture change for the expander — the keystroke path is no longer the default. Bumping the minor signals that the failure modes (and therefore the user-visible behaviour) shift. The fallback path keeps full backward compatibility — every app that worked in 0.2.x still works in 0.3.0, just often via a cleaner mechanism.

### Tests

`cargo test --workspace`: **74 → 84 green** (+7 word-boundary, +3 UTF-16). `pnpm test`: 53 unchanged.

## [0.2.12] — 2026-04-28

### Changed

- **Backup Export / Import moved to the Settings tab.** Lived under the Notes tab's sidebar since v0.2.6, but conceptually belonged with the rest of the app-level configuration. The Notes tab keeps **+ New Note** and **Clear All**; everything backup-related is now under the new **Backup & restore** section in Settings. — *#feat(settings)*
- **Selective export.** Three checkboxes — *Clipboard history*, *Snippets*, *Notes* — let you choose which sections land in the file. All checked by default; unchecking any of them writes an empty array for that section in the JSON. Intended use: share snippets without leaking your clipboard history.
  - Backend: new `backup::ExportOptions { include_history, include_snippets, include_notes }` with `::all()` / `::default()` constructors. Both `export_backup` and `save_backup_to_file` IPC commands take three optional flags (default `true`). Existing callers stay backward-compatible.
  - Frontend: `BackupExportOptions` interface in `ipc.ts`. `exportBackup()` / `saveBackupToFile(path, opts)` accept the same fields.
  - 3 new Rust unit tests (`export_with_only_snippets…`, `export_with_all_off…`, `export_options_default…`). Backend total: 71 → **74 green**.

### Fixed

- After an Import, the Notes / Snippets / History tabs now refresh immediately. The Settings panel takes an `onBackupImported` prop from `App.tsx` that re-fires the three list hooks (`refreshHistory`, `refreshSnippets`, `refreshNotes`) once the merge returns.

## [0.2.11] — 2026-04-26

### Fixed

- **Crash on hotkey / Test now: `EXC_BREAKPOINT` from `_dispatch_assert_queue_fail`.** The text-expander dispatched `enigo` work onto a worker thread (`std::thread::spawn` in `register_expander`, plus the IPC handler thread for `trigger_expand_at_cursor` / `diagnose_expand_at_cursor`). On macOS, enigo's `Key::Unicode(...)` mapping calls `TSMGetInputSourceProperty` (Text Services Manager) which **asserts main-thread**. Calling it from any other thread fires a libdispatch assertion and aborts the process with SIGTRAP. Confirmed by three crash reports today: `inspector-rust-2026-04-26-070927.ips`, `…-070931.ips`, etc — all ended at `enigo::macos_impl::keycode_to_string` from a worker thread.
  - **Fix:** all three call sites now dispatch the expand cycle to the main thread via `AppHandle::run_on_main_thread`. The hotkey path is fire-and-forget; `diagnose_expand_at_cursor` ferries the result back through an `mpsc::channel`. The popup is hidden during the cycle, so the ~290 ms main-thread block is invisible to the user.

## [0.2.10] — 2026-04-26

### Fixed

- **macOS Accessibility re-grant loop is finally broken.** Real root cause this time, not symptoms: macOS Tahoe (26.x) binds the TCC Accessibility grant to the tuple `(bundle id, cdhash)`. `scripts/install-macos.sh` previously ran `codesign --force` on every install — even when the user re-installed an *unchanged* binary — which embedded a fresh CMS timestamp into the signature blob and produced a new cdhash. macOS then dropped the prior grant, prompting again. — *#fix(macos)*
  - **Idempotent install.** The script now SHA-256 compares the freshly built binary at `target/release/bundle/macos/InspectorRust.app/Contents/MacOS/inspector-rust` against the currently installed binary at `/Applications/InspectorRust.app/Contents/MacOS/inspector-rust`. If they're identical (and the bundle identifier already matches), the script **skips both `cp` and `codesign`** entirely — your install is preserved verbatim, the cdhash stays stable, and your TCC grant survives. Net effect: rebuilds without source changes never ask you to re-grant.
  - **Cleaner re-sign output.** When source *did* change, the script now prints both old and new SHA-256 prefixes plus the resulting cdhash, with an explicit "TCC grant must be re-given" warning so you know what to expect.
- **Wrong entitlement removed.** `com.apple.security.automation.apple-events` was misleadingly attached "for enigo to simulate paste" but actually covers AppleScript automation (NSAppleEvent / OSAScript), not `CGEventPost`-style synthetic input. Worse, on macOS Tahoe with Hardened Runtime its presence can trigger an unrelated TCC "Automation" prompt and confuse the permission flow. Removed from `macos/src-tauri/entitlements.plist`. The remaining three entitlements (`allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`) correctly cover WebKit / Tauri plugin loading.

### Added

- **Auto-restart prompt after grant detected.** The Settings panel's polling loop now distinguishes the false→true transition of `accessibility_granted`. When it fires, an inline emerald-bordered prompt appears: **"Access detected — one more step"** with a **Restart now** button. Click → Inspector Rust spawns a fresh `/Applications/InspectorRust.app` process via `open -n` and exits cleanly. The new instance picks up the just-granted TCC state correctly (the running process couldn't, because macOS caches `AXIsProcessTrusted()` per-process). Total post-grant flow: ~30 seconds, one click. — *#feat(settings)*
  - New `relaunch_app` IPC command in `core/rust-lib/src/commands.rs`.
  - `relaunchApp()` wrapper in `core/frontend/src/lib/ipc.ts`.
- **"Why does this keep happening?" disclosure** in the amber banner of the Settings panel, explaining the cdhash binding in plain language so users understand the constraint instead of feeling gaslit by the OS.

### Changed

- **`[profile.release]`** at the workspace root: `codegen-units = 1`, `lto = true`, `strip = "debuginfo"`, `opt-level = 3`. Won't make Rust release builds fully byte-reproducible, but reduces non-determinism so the SHA-256 idempotency check has a fighting chance for trivial source changes.
- **`scripts/install-macos.sh`** — full restructure with helper functions (`bin_sha256`, `cdhash`, `current_identifier`, `kill_running`, `resign_app`, `reset_tcc`) and clearer printed status. The script's docstring at the top now accurately describes the cdhash binding and how the idempotent path works.
- **`macos/README.md`** "Why the dialog re-appears" section rewritten with the honest truth instead of the previous wishful "Sequoia and earlier accept this; later releases may still re-prompt." Now says: every meaningful rebuild requires re-grant on Tahoe; the script + auto-restart prompt make it bearable; the only permanent fix is an Apple Developer ID.

### Verification recipe

```bash
# 1) idempotent rebuild preserves grant
bash scripts/install-macos.sh        # initial install
# … grant Accessibility once via Settings panel banner …
bash scripts/install-macos.sh        # re-run with no source changes
#   ⇒ prints "Binary unchanged — keeping existing install"
#   ⇒ green banner stays green; Diagnose works without intervention

# 2) source change triggers single re-grant
echo "// touch" >> core/rust-lib/src/lib.rs
bash scripts/install-macos.sh
#   ⇒ prints "Binary changed — full reinstall"
#   ⇒ amber banner appears in Settings tab
#   ⇒ click Open System Settings → enable toggle → switch back
#   ⇒ green "Restart now" prompt appears within 1 s
#   ⇒ one click → app relaunches → Diagnose works
```

## [0.2.9] — 2026-04-26

### Added

- **Accessibility status badge in the Settings panel** — green when Inspector Rust has macOS Accessibility access, amber when it doesn't, with an inline explainer of what to do. Polled once per second while not granted, so the badge flips to green within ~1 s of the user toggling Inspector Rust on in System Settings — no panel reload needed. — *#feat(settings)*
- **`Test now` button** in the Settings panel — runs the full expand-at-cursor cycle without using the hotkey after a 2-second grace period (long enough to switch back to the source app and place the cursor after an abbreviation). Lets you tell whether the *hotkey* is the problem or the *expansion logic* is. Wired through the existing `trigger_expand_at_cursor` IPC.
- **`get_accessibility_status` Tauri command** + `ExpanderConfig.accessibility_granted` field — backed by macOS `AXIsProcessTrusted()` via FFI to `ApplicationServices.framework`. Returns `true` unconditionally on Windows / Linux, where synthetic input is either ungated or gated by a different mechanism.

### Fixed

- **`scripts/install-macos.sh`** — new helper that builds + re-signs Inspector Rust with a stable ad-hoc identifier (`io.celox.inspector-rust`) before copying into `/Applications`. Without an Apple Developer ID, every fresh `pnpm build:macos` produced a *random* identifier (e.g. `inspector-rust-c64f925d…`); macOS TCC then treated the rebuild as a brand-new app and discarded the previous Accessibility grant. The script's stable identifier lets the grant survive across rebuilds (where macOS allows bundle-id matching), and `--reset` runs `tccutil reset` to wipe stale carcass entries when needed.
- **macOS README** — new "Why the dialog re-appears after every rebuild" section explaining TCC binding to code-signature, plus how to use `install-macos.sh`.

## [0.2.8] — 2026-04-26

### Fixed

- **Expander hotkey capture failed for the `^` key on German ISO macOS keyboards.** WebKit reports the top-left key (`^`/`°`) as `event.code = "IntlBackslash"`, but the Tauri `tauri-plugin-global-shortcut` parser (`Shortcut::from_str`) maintains a hand-written allow-list that doesn't include any `Intl…` codes — the captured combo `Alt+IntlBackslash` was rejected with `UnsupportedKey("IntlBackslash")`. Two-part fix: — *#fix(expander)*
  - **Frontend** (`HotkeyCapture.tsx`) — new `normalizeCode()` maps WebKit's `IntlBackslash` back to `Backquote` (the layout-stable W3C name; same Carbon virtual keycode `kVK_ANSI_Grave` = 0x32 the OS will see at hotkey time).
  - **Backend** (`hotkey::parse_shortcut`) — replaces the plugin's narrow parser with our own. Routes the code token through `keyboard_types::Code::from_str`, which understands the **full** W3C `KeyboardEvent.code` spec. Future-proofs against other gaps in the plugin's allow-list (`IntlBackquote`, `IntlRo`, `IntlYen`, less-common media keys, …).
  - 9 new unit tests for the parser (modifier aliases, `IntlBackslash` accept, single-key, error cases). Backend tests: 62 → **71 green**.
- **HotkeyCapture button never recorded on macOS.** Safari/WebKit does **not** focus a `<button>` on click, so the button-level `onKeyDown` never fired. The capture indicator stayed at "Press a key combination…" forever. Fix: while capturing, attach a window-level keydown listener in *capture phase* — wins over the global keyboard-nav hook (which would otherwise consume Esc as "close popup"). — *#fix(settings)*
- **Search bar placeholder + Notes/Snippets/Settings titles ran behind the absolutely-positioned tab strip.** With four tabs (after Settings was added in 0.2.7) the strip overlapped the input. Fix: reserve `pr-[260px]` on the search bar and on the inactive-tab title row, tighten tab buttons to `px-2 whitespace-nowrap`, shorten the placeholder to `Search or calculate…`. — *#fix(ui)*

### Added

- **Per-row delete + Clear all** for clipboard history. Hover any clip row in the History tab → trash icon appears next to the bookmark icon → one click removes that single entry. A new toolbar at the top of the history list shows the clip count and a **Clear all** button (with `window.confirm` guard) for nuking everything at once. Wired through the existing `delete_entry` / `clear_history` IPC commands. — *#feat(history)*

### Changed

- `useClipboardHistory` now exposes its `refresh` callback to `App.tsx` so the list refetches immediately after delete/clear-all instead of waiting for the next `clipboard-changed` event.

## [0.2.7] — 2026-04-25

### Added

- **System-wide text expander.** Type a snippet abbreviation in any text field — code editor, browser, mail client, Slack — then press the configured hotkey, and Inspector Rust replaces the abbreviation in place with the snippet body. Default hotkey is `Alt+Backquote` (the `^` key on a German keyboard, ` on US). Disabled by default — opt in from the new **Settings** tab. — *#feat(expander)*
  - **How it works:** the popup stays out of the way. Inspector Rust synthesizes `Cmd/Ctrl+Shift+←` (select previous word) → `Cmd/Ctrl+C` (copy), looks the captured word up in the snippets table via the new `find_by_exact_abbreviation` (case-sensitive first, case-insensitive fallback), writes the body to the clipboard, and synthesizes `Cmd/Ctrl+V`. The user's clipboard is saved before the cycle and restored after.
  - **Trigger semantics, not silent watch.** No global keylogger — you decide when to expand.
  - **Configurable hotkey.** New **Settings** tab → click the hotkey field → press your combination (Backspace clears, Esc cancels). The string is stored in the new `settings` SQLite table and re-registered with the OS via `tauri-plugin-global-shortcut`. Bad combinations are rejected before the previous registration is touched, so you can't accidentally lose your hotkey to a typo.
  - **Cross-platform.** macOS / Windows / Linux X11 work the same. Linux Wayland depends on the compositor's global-shortcut portal (GNOME/KDE OK; sway-flavoured stacks may not).
  - Full reference: [`docs/text-expander.md`](./docs/text-expander.md).
- **Settings tab** in the popup, alongside History · Snippets · Notes. Designed to grow — first home for the expander toggle + hotkey capture; future settings (capture pause defaults, image-size cap, …) will land here.
- **`settings` SQLite table** — new key/value store via `core/rust-lib/src/settings.rs`. Idempotent migration; created on first launch of v0.2.7.
- **`HotkeyCapture` React component** that converts a `KeyboardEvent` into the W3C-code shortcut format the global-shortcut plugin's parser expects (`Modifier+...+Code`).
- **14 new Rust unit tests** — settings store roundtrip (6), `snippets::find_by_exact_abbreviation` semantics (5), expander helpers (3). `cargo test --workspace`: 48 → **62**.

### Changed

- IPC surface gains `get_expander_config`, `set_expander_config`, `trigger_expand_at_cursor`. The latter is a programmatic alternative to the hotkey — useful for testing and for any future tray-menu entry.
- `hotkey.rs` gains `ExpanderShortcutState` (Tauri-managed) and `register_expander(...)`, which idempotently swaps the previously-registered expander shortcut. Runs the actual expansion on a worker thread so the global-shortcut callback returns instantly (avoids platform-specific deadlocks).

### Caveats — what *won't* work cleanly

These are documented in [`docs/text-expander.md`](./docs/text-expander.md), surfaced in the Settings panel's "How it works" disclosure:

- **Terminals** (iTerm2, kitty, gnome-terminal) sometimes interpret `Cmd/Ctrl+Shift+←` as a pane-switch / mark-selection — the expander may grab the wrong "word" or nothing at all.
- **Password fields** in many apps refuse synthetic paste; the abbreviation gets selected but the body never lands.
- **Linux Wayland** in restrictive compositors blocks global shortcuts entirely.
- **Image / files snippets** are not supported by the expander (the orchestration only handles text). This is intentional for v1.

## [0.2.6] — 2026-04-25

### Added

- **Notes — a third tab for persistent, categorized clipboard items.** Notes live in their own SQLite table and are *not* affected by the 1 000-entry pruning of the clipboard history, so they're the right place for things you want to keep. — *#feat(notes)*
  - Three-pane layout: **Categories sidebar** (with note counts per category, plus virtual `All` and `Uncategorized` groups), **note list**, and **detail/edit pane**.
  - **Free-form categories** — typing a new category name in the edit form auto-creates it; the input has a `<datalist>` for autocomplete from existing categories.
  - **Editable bodies** for `text`, `html`, `rtf` notes; `image` and `files` notes are read-only (you can still rename them and change category). The detail pane renders images inline and shows file paths as a list.
  - **Paste from a note** preserves the original content type — image notes paste as images, HTML notes paste as HTML, etc.
- **Star button on history rows** — hover any clipboard entry in the History tab and the bookmark icon appears next to the timestamp; one click promotes the entry to a note in the `Uncategorized` bucket. The note is decoupled from the clip thereafter, so even if the clip gets pruned out of history, the note stays.
- **Full-app backup** — Notes tab toolbar gets `Export…` and `Import…` actions wired through `tauri-plugin-dialog`. Export writes a single pretty-printed JSON file (`{ version, exported_at, history, snippets, notes }`); import merges that file back into the live database with sensible per-table semantics:
  - **Snippets** — upsert by `abbreviation` (existing rows are overwritten).
  - **History** — upsert by SHA-256 hash; duplicates just bump `last_used_at`, new rows respect the existing 1 000-entry cap.
  - **Notes** — appended verbatim with original timestamps preserved (no natural dedup key, so re-importing the same backup creates duplicates — use Clear All first if you want a clean replace).
- **`Clear All` for notes**, with a `window.confirm` guard.
- **Tray menu entry “Manage Notes”** — opens the popup directly on the Notes tab via a new `open-notes-tab` event.
- **15 new Rust unit tests** for the notes module (CRUD, categories, save_from_clip, image-note read-only update) and the backup module (roundtrip into empty db, merge into populated db, version-rejection guard, replace-all). `cargo test --workspace` is now **48 → was 33**.

### Changed

- `paste.rs::write_to_clipboard` was refactored to take primitives `(content_type, data, text)` instead of a `&ClipEntry`, exposed via the new public `paste::paste_payload(...)`. This lets the `paste_note` IPC command paste any content type without needing to construct a fake `ClipEntry`.
- New IPC commands wired into `invoke_handler`: `list_notes`, `list_note_categories`, `save_clip_as_note`, `create_note`, `update_note`, `delete_note`, `clear_notes`, `paste_note`, `export_backup`, `save_backup_to_file`, `import_backup`.
- New permissions in both shells' `capabilities/default.json`: `dialog:allow-save` (for the export file picker).

### Database

- New table on first launch (idempotent `CREATE TABLE IF NOT EXISTS`):
  ```sql
  CREATE TABLE notes (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      content_type TEXT NOT NULL,
      content_text TEXT NOT NULL DEFAULT '',
      content_data TEXT NOT NULL DEFAULT '',
      title        TEXT NOT NULL DEFAULT '',
      category     TEXT NOT NULL DEFAULT '',
      byte_size    INTEGER NOT NULL DEFAULT 0,
      created_at   INTEGER NOT NULL,
      updated_at   INTEGER NOT NULL
  );
  ```
  Indexed on `category` and `updated_at DESC`.

## [0.2.5] — 2026-04-25

### Added

- **Inline calculator in the search field** — Alfred-style. As you type, Inspector Rust evaluates the input as a math expression and shows the result as the top list item; press Enter to paste the result into the previously active app. Bare numbers (`42`) and plain text (`hello`) are ignored; only inputs with at least one operator, function call, or named constant trigger calc mode. A leading `=` forces evaluation (so `=42` or `=pi` displays a result for a single literal). — *#feat(calc)*
  - Supported operators: `+ - * / % ^` (power is right-associative), unary `+`/`-`, parens.
  - Supported numbers: integers, decimals (`0.5`, `.5`), scientific (`1e3`, `1.5e-2`), digit grouping (`1_000`).
  - Constants: `pi` / `π`, `tau`, `e`.
  - Functions: `sqrt`, `cbrt`, `abs`, `sign`, `floor`, `ceil`, `round`, `ln`, `log` (base 10), `log2`, `exp`, `sin`/`cos`/`tan` (radians), `asin`/`acos`/`atan`/`atan2`, `sinh`/`cosh`/`tanh`, `min`, `max`, `pow`, `mod`.
- **`paste_text(text)` Tauri command** — generic "compute & paste" entry point used by the calculator (and available for future flows like unit-conversion / date-math). Hides the popup, writes `text` to the clipboard, and synthesizes Cmd+V / Ctrl+V via `enigo`, same as the existing snippet-paste path.
- **27 new vitest cases** for `tryEvaluate` and `formatResult` covering precedence, right-associative power, parens, decimals + scientific notation, every supported function/constant, `=`-forced evaluation, and rejection of plain numbers / malformed input. (`pnpm test`: 24 → 51 frontend tests.)

### Changed

- **Search field rebranded as a general input.** Placeholder is now `Search history or type an expression (2+2, sqrt(16), …)`. The leading icon is a chevron by default and switches to a calculator glyph the moment the input parses as a math expression — making the field read as an entry box, not just a search box.
- New `CalcEntry` variant in `ListEntry`; `HistoryItem` renders calc rows with a `calc` chip and `expr = result` formatting in monospace, `PreviewPanel` shows a centered large `= result` view.

## [0.2.4] — 2026-04-25

### Fixed

- **Paste did not land in the previously active app on macOS.** Hiding only the popup window left Inspector Rust (an `Accessory`-policy app) in a state where the OS could not reliably hand key focus back to the prior frontmost app, so `enigo`'s synthesized `Cmd+V` either dropped on the floor or arrived back at Inspector Rust. — *#fix(paste)*

### Changed

- `hotkey::hide_popup` now also calls `AppHandle::hide()` on macOS (no-op on other platforms), which invokes `NSApplication.hide(nil)` and forces the OS to restore the prior frontmost app as key window. The popup window is hidden first, then the app.
- The settle delay between clipboard write and the synthesized paste keystroke is now platform-specific: **120 ms on macOS** (was 50 ms — `NSApp.hide()` takes a frame or two), unchanged 50 ms on Windows / Linux.

## [0.2.3] — 2026-04-25

### Fixed

- **Import button appeared to crash the app on macOS.** When the native file dialog (`NSOpenPanel`) opened, the popup window lost focus, which fired our existing `Focused(false)` window event → `hide_popup()` ran → the popup vanished. The dialog often stayed half-up but with its parent gone, the user perceived the whole app as having crashed. — *#fix(snippets)*

### Added

- New `UiState { suppress_hide: AtomicBool }` shared state and IPC command `set_suppress_hide(suppress: bool)`. The Snippets-tab Import handler now wraps the `dialog.open()` call in `setSuppressHide(true) … finally setSuppressHide(false)` so the popup stays put while NSOpenPanel owns focus.
- `core/rust-lib/src/ui_state.rs` — new module owning the shared UI flag.

### Changed

- The popup's `Focused(false)` handler in `lib.rs` consults the suppress flag before calling `hide_popup`. Default behaviour (auto-hide on click-outside, Esc, alt-tab) is unchanged.

## [0.2.2] — 2026-04-25

### Fixed

- **JSON snippet import was broken on macOS.** The 0.2.1 implementation used a hidden `<input type="file">` triggered by `.click()` from React. WKWebView (Tauri's macOS renderer) does not reliably surface a native file picker for hidden inputs in this pattern, so the Import button appeared to do nothing on macOS. — *#fix(snippets)*

### Changed

- **Switched the snippet-import file picker to `tauri-plugin-dialog`.** The Import button now opens the native NSOpenPanel / Win32 OpenFileDialog via `@tauri-apps/plugin-dialog`'s `open()`, with a `.json` filter and a localized "Select snippets JSON file" title. Selected path is read in Rust (`std::fs::read_to_string`) and parsed by the existing `import_from_json` pipeline.

### Added

- New IPC command `import_snippets_from_file(path: String) -> ImportResult` (in addition to the existing `import_snippets(json: String)` which is still used by tests).
- `tauri-plugin-dialog` workspace dep + capability permission `dialog:allow-open` in both the Windows and macOS shells.
- Import button shows "Importing…" while the dialog/import is in flight.
- **5 themed example JSON files** under `docs/examples/snippets/` — `getting-started.json` (3 entries), `signatures.json` (4), `dev.json` (8), `markdown.json` (5), `wrapped-form.json` (2, demonstrates the `{ snippets: [...] }` shape). Each is a stand-alone, ready-to-import file; the folder has its own `README.md` indexing them and showing how to merge multiple files via `jq -s 'add'`.
- `docs/snippets-import.md` extended with a Tips & anti-patterns section.
- Root `README.md` Snippet-import section now lists all example files in a table instead of a placeholder code block.

## [0.2.1] — 2026-04-25

### Added

- **JSON snippet import** — bulk-load snippets from a `.json` file via **Snippets → Import** in the popup. Existing abbreviations are upserted in place, so re-importing the same file is idempotent. Both `[…]` (bare array) and `{ "snippets": [...] }` (wrapped) shapes are accepted; per-row failures are collected in the result without aborting the whole import. See [`docs/snippets-import.md`](./docs/snippets-import.md) for the schema and [`docs/snippets-example.json`](./docs/snippets-example.json) for a sample. — *#feat(snippets)*
- **`macos/README.md`** with installation, Gatekeeper bypass, Accessibility-permission setup, and troubleshooting (DMG bundle failures, missing tray icon).
- **`docs/snippets-import.md`** — full reference: file format, field semantics, sample-file walkthrough, manual export recipe via `sqlite3` + `jq`, IPC surface, test matrix.
- **`CHANGELOG.md`** (this file).
- **6 new Rust unit tests** for the snippet import path (`cargo test --workspace`: 27 → 33).

### Fixed

- **CI was failing** with `ERR_PNPM_OUTDATED_LOCKFILE` because `macos/package.json` (added in 0.2.0) declared `@tauri-apps/cli` without a lockfile refresh. The lockfile is now in sync. — *#fix(ci)*
- **macOS build was broken** in 0.2.0:
  - `tauri.conf.json` declared `macOSPrivateApi: true` but the corresponding `tauri/macos-private-api` cargo feature was not enabled — `tauri-build` aborted. — *#fix(build)*
  - `app.set_activation_policy(...)` was wrapped in `if let Err(e) = …`, but the function returns `()`, not `Result`. The whole crate failed to typecheck on macOS. — *#fix(build)*
- **Multi-monitor popup placement** — the popup occasionally opened in the bottom-right of the active monitor and could even extend past the screen edge, most reliably reproducible on mixed-DPI setups (MacBook Retina + external display). The show/position pipeline was restructured: pick cursor monitor first, park the hidden window onto it, **then** `show()` + `set_focus()` (so `outer_size()` returns a real value), then re-resolve the monitor and finally call new helper `clamp_into_monitor()` which hard-clamps `x`/`y` to the monitor's bounds so the window can never overflow. — *#fix(hotkey)*

### Changed

- **`README.md`** — added a Multi-monitor placement subsection, surfaced the JSON-import feature, refreshed the repo layout to include `macos/` and the new docs, bumped test counts (24 frontend, 33 Rust).
- **`.gitignore`** — ignore `.claude/` (per-machine agent session state).

### Known issues

- The macOS DMG bundling step (`bundle_dmg.sh`) occasionally fails on busy disks (FileVault background indexing, Time Machine snapshot in progress). The `.app` itself is built first and is unaffected — see [`macos/README.md` § Troubleshooting](./macos/README.md#troubleshooting).
- macOS builds are **arm64 only** (Apple Silicon). Intel-Mac users need to build from source with `--target x86_64-apple-darwin`.
- Bundles are **not Apple-signed** — Gatekeeper will refuse to open on first launch. Workarounds documented in `macos/README.md`.

## [0.2.0] — 2026-04-24

### Added

- **macOS bundle shell** under [`macos/`](./macos) — DMG + `.app` targets, `entitlements.plist`, capabilities, thin `main.rs` reusing `inspector-rust-core`.
- **Text expander** ("snippets") — abbreviations (e.g. `mfg`) with optional title and body. Matching snippets appear at the top of the History list when you type their abbreviation; Enter pastes the body. Dedicated **Snippets** tab for create/edit/delete, **Manage Snippets** entry in the tray menu.
- **GitHub Actions CI** — Rust + frontend tests on every push/PR ([`ci.yml`](./.github/workflows/ci.yml)).
- **GitHub Actions release** — builds Windows MSI/EXE and publishes a GitHub Release on `v*` tags ([`release.yml`](./.github/workflows/release.yml)).
- **Frontend unit tests** — vitest + happy-dom + @testing-library/react (`Footer`, `format` helpers — 24 tests).
- **Rust unit tests** — in-memory SQLite tests for `db` (insert/dedupe/list/touch/prune — 27 tests).
- README badges, icon header, polished layout.

### Known issues (resolved in 0.2.1)

- macOS build broken (`macos-private-api` cargo feature missing, `set_activation_policy` type mismatch). Fixed in 0.2.1.
- CI failing due to stale `pnpm-lock.yaml`. Fixed in 0.2.1.

## [0.1.0] — 2026-04-23

### Added

- Initial release. Windows-first clipboard history manager.
- Global hotkey `Ctrl+Shift+V` opens a frameless, always-on-top popup centered on the cursor's monitor.
- Captures **text**, **RTF**, **HTML**, **images** (≤ 5 MB, base64 PNG), and **file lists** via real OS clipboard change events (no polling).
- Fuzzy search (`fuse.js`, threshold 0.4) over preview text.
- Auto-paste with `enigo` (simulates `Ctrl+V` after the popup hides).
- SQLite history at `%APPDATA%\InspectorRust\history.db`, deduped on SHA-256, capped at 1 000 entries.
- System tray menu: Open · Pause Capture · Clear History · Start with Windows · Quit.
- pnpm + Cargo workspaces with shared [`core/`](./core) and [`win/`](./win) bundle shell.

[0.5.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.5.1
[0.5.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.5.0
[0.4.2]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.4.2
[0.4.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.4.1
[0.4.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.4.0
[0.3.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.3.1
[0.3.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.3.0
[0.2.12]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.12
[0.2.11]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.11
[0.2.10]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.10
[0.2.9]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.9
[0.2.8]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.8
[0.2.7]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.7
[0.2.6]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.6
[0.2.5]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.5
[0.2.4]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.4
[0.2.3]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.3
[0.2.2]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.2
[0.2.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.1
[0.2.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.0
[0.1.0]: https://github.com/pepperonas/inspector-rust/commits/main
