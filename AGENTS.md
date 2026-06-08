# AGENTS.md

Compact guidance for AI agents. Detailed architecture is in `CLAUDE.md` — read it
for IPC patterns, DB schema, feature internals, and platform-specific behaviour.

## Quick commands

```bash
pnpm install                  # once after clone
pnpm dev:win                  # hot-reload dev (Windows)
pnpm check                    # full static analysis: clippy + tsc + eslint (MUST pass)
pnpm test                     # frontend vitest (single run)
cargo test --workspace        # all Rust unit tests (in-memory SQLite, no services)
pnpm typecheck                # tsc --noEmit
pnpm lint                     # eslint src
cargo clippy --workspace --all-targets -- -D warnings
```

**Single frontend test file:** `pnpm --filter inspector-rust-frontend vitest run src/lib/foo.test.ts`

**Verification order:** clippy → typecheck → lint → test (mirrors CI).

## Architecture (what agents get wrong)

- **All Rust logic lives in `core/rust-lib/`** — the three platform shells (`win/`, `macos/`,
  `linux/`) are 2-line `main.rs` wrappers. Never add logic to a platform shell.
- **Frontend is shared** — `core/frontend/` is the single React app used by all platforms.
- **IPC contract (4 files):** logic in `core/rust-lib/src/*.rs` → `#[tauri::command]` in
  `commands.rs` → register in `lib.rs` `invoke_handler![]` → typed wrapper in
  `core/frontend/src/lib/ipc.ts`. All four must stay in sync.
- **`ListEntry` union in `lib/types.ts`** is the central data type for the history view.
  Adding a new entry kind requires updating: the union, the assembly in `App.tsx`
  (`combined`), and the rendering in `HistoryItem.tsx`.

## Conventions that differ from defaults

- **No `any`** — ESLint enforces `@typescript-eslint/no-explicit-any: "error"`.
- **Co-located tests** — `src/lib/foo.ts` → `src/lib/foo.test.ts` (vitest + happy-dom).
- **Rust tests use `Connection::open_in_memory()`** — no temp files, no test fixtures on disk.
- **Commit style:** `type(scope): description (vX.Y.Z)` — conventional-commits with version tag.
- **Version is synced** in: root `package.json`, workspace `Cargo.toml`, `core/frontend/package.json`,
  and each platform's `tauri.conf.json`. Bump all or none.
- **Hidden triggers** (games, `opener`, `2fa`, `bpm`) are deliberately NOT in the `COMMANDS`
  catalogue — they must never appear in autocomplete. Check `CLAUDE.md` for the full list.

## Gotchas

- **Windows runtime-unverified** — several Rust modules (brightness, auto_expand win impl,
  screenshot capture modes) are compile-clean but untested on real Windows. Note this in
  commits touching those paths.
- **`pnpm check` uses bash** — on Windows, run via Git Bash or WSL, or run the three
  commands individually (clippy, typecheck, lint).
- **Release LTO builds are slow** (~5 min) — use `pnpm dev:win` for iteration, not `build:win`.
- **Tauri capabilities** — each platform has a `capabilities/default.json` that must list
  any new window labels (e.g. `screenshot-pin-*` glob). Forgetting this causes silent
  IPC failures at runtime.
- **Field-level encryption** — `entries.content_text`, `entries.content_data`, `snippets.body`,
  `notes.content_text/data`, and TOTP secrets are AES-256-GCM encrypted. Values prefixed
  `"v1:"` are encrypted; others are legacy plaintext (auto-migrated on read). Never store
  secrets in plaintext columns.
- **macOS TCC grants** — three independent surfaces (Accessibility, Screen Recording,
  Automation→Finder). The stable self-signed cert + `codegen-units=1` LTO keeps grants
  across rebuilds. Changing the signing identity invalidates all grants.
- **`VITE_IR_MEME`** env var (default on) — set to `0` for meme-less builds. Only gates
  the frontend; Rust `meme.rs` compiles regardless.

## Toolchain versions (per CI)

- **pnpm 10**, **Node 20**, **Rust stable** (2021 edition)
- Tauri 2, React 19, Vite 7, TypeScript 5.8, Tailwind CSS 4, Vitest 3

## What NOT to touch without asking

- `core/rust-lib/models/u2netp.onnx` — embedded ML model (~4.5 MB), do not regenerate.
- `core/frontend/src/lib/openers-data.ts` — auto-generated from a remote DB export.
- `core/frontend/src/lib/pwgen-dict.ts` — static word list for password generation.
- Platform `tauri.conf.json` security scopes and capabilities — changes affect TCC and
  sandboxing.
