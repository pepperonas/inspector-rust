# Bug Report

> Phase D. Generated 2026-06-06 · base version 0.61.0. Static analysis +
> trade-off audit. No Critical bug was found that warrants an immediate
> in-phase fix; the items below are graded and carry a recommended fix.

## Static analysis state (green-gate)

| Check | Result |
|---|---|
| `cargo test --workspace` | 🟢 338 passed |
| `pnpm test` (vitest) | 🟢 493 passed |
| `pnpm typecheck` | 🟢 |
| `pnpm lint` (eslint) | 🟡 17 problems (15 errors, 2 warnings) — all **pre-existing** |
| `cargo clippy` | ⚪ not installed on the build host (`rustup component add clippy`) |

The eslint errors are the eslint-plugin-react-hooks v6 `react-hooks/refs`
("Cannot access refs during render") class in `PreviewPanel.tsx`,
`BpmDetector.tsx`, and `ScreenshotEditor.tsx:412`. They predate this work and
are **down from 22 → 17** since Phase B2 hoisted the editor's `ToolBtn` out of
render. Not behavioural bugs, but worth clearing in a dedicated pass.

## Findings

### D-1 · Direct-slot expander blind-deletes N backspaces · Sev: Medium · `expander.rs::paste_snippet_body`
A direct hotkey deletes `abbreviation.chars().count()` characters before
pasting, **whether or not the user typed the abbreviation**. Firing the hotkey
with the cursor in arbitrary text silently eats N chars. Documented trade-off
(works in terminals because it reads nothing), but it is a data-loss footgun.
**Fix idea:** optional per-slot "delete abbreviation" toggle (default off), or
an AX read on platforms that support it before deleting. OS: all.

### D-2 · Notes import has no dedup key · Sev: Low · `backup.rs`
Re-importing a backup appends notes verbatim → duplicates on every import
(snippets upsert by abbreviation, history dedupes by hash; notes don't).
**Fix idea:** dedup notes by `(title, content hash)` on import. OS: all.

### D-3 · Wayland global shortcuts often don't fire · Sev: Medium (Linux) · `lib.rs`/`cli_dispatch.rs`
Known GNOME/Wayland limitation; mitigated by the CLI-flag + desktop-shortcut
fallback. Registration is already non-fatal. **No code bug** — ensure the
Linux README keeps the workaround prominent. OS: Linux.

### D-4 · macOS Accessibility cached per-process → relaunch needed after grant · Sev: Low (UX) · `expander.rs`
`AXIsProcessTrusted` is cached for the process lifetime, so a freshly-granted
permission isn't usable until relaunch. The Settings panel already detects the
false→true transition and offers a relaunch prompt. Verify every entry point
(auto-expand, input-lock, finder) routes the user to that same prompt. OS: macOS.

### D-5 · Auto-expansion ignores input for ~150 ms after each expansion · Sev: Low · `auto_expand.rs`
The `INJECTING` guard (macOS) stays set for a 150 ms grace window so our own
synthetic keystrokes aren't re-fed. Real keystrokes typed in that window are
dropped from the buffer (not from the app — they still land; they just don't
contribute to a *next* abbreviation match). Practically invisible at human
typing speed; noted for completeness. OS: macOS (Windows uses the
`LLKHF_INJECTED` flag and isn't affected). **Runtime-unverified.**

### D-6 · Windows `ToUnicodeEx` in the auto-expand hook can disturb dead keys · Sev: Medium (Windows, theoretical) · `auto_expand.rs`
`ToUnicodeEx` has the documented side effect of consuming dead-key state.
Calling it from the low-level hook to decode the typed char could interfere
with compose sequences (e.g. `^` + `e` → `ê`) on some layouts. **Needs a real
Windows test.** Mitigation if it bites: pass the proper key state and/or
re-inject the dead key, or decode without clearing state. OS: Windows.

### D-7 · Windows screenshot/system/md2pdf paths are runtime-unverified · Sev: Unknown (Windows) · multiple
All `#[cfg(windows)]` code added in Phases A/B/C/Markdown compiles cleanly but
has **not been exercised on real Windows hardware** (build host is macOS).
Specifically: `WH_KEYBOARD_LL` auto-expand, GDI fullscreen/window capture,
`shutdown`/`rundll32`/`keybd_event` system commands, Edge-headless md2pdf.
**Action:** a Windows smoke-test pass before the next Windows release.

### D-8 · `clean` confirmation uses `window.confirm` summary, not a per-file preview · Sev: Low (UX) · `App.tsx`/`cleaner.rs`
The safety guarantees hold (dry-run scan → confirm → re-validated execute), but
the preview is a count + per-category byte breakdown in a native confirm, not a
scrollable file list. **Enhancement, not a bug:** a richer preview overlay.

## Priority

| ID | Sev | Recommended for |
|---|---|---|
| D-6 | Medium (Win) | Verify in the Windows smoke-test (D-7) |
| D-1 | Medium | Next maintenance pass (opt-in toggle) |
| D-3 | Medium (Linux) | Doc-only; already mitigated |
| D-7 | Unknown | Windows smoke-test before release |
| D-2, D-4, D-5, D-8 | Low | Backlog |

No Critical/High bug requiring an immediate fix was found in the new code; the
new modules ship with unit tests (`auto_expand` 24, `region_picker` 2,
`editor-geometry` 10, `cleaner` 14).
