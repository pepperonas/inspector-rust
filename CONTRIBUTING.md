# Contributing to Inspector Rust

Thanks for considering a contribution! Inspector Rust is a small, opinionated app — we'd rather merge a tight fix than a sprawling refactor.

## Quick start

```bash
git clone https://github.com/pepperonas/inspector-rust.git
cd inspector-rust
pnpm install               # workspace install (Cargo + pnpm)
```

Run the app in dev mode:

```bash
pnpm dev:macos             # macOS host
pnpm dev:win               # Windows host
```

For platform-specific build prerequisites see [`win/README.md`](./win/README.md) and [`macos/README.md`](./macos/README.md).

## Project layout (TL;DR)

- **`core/frontend/`** — React 19 + TS + Tailwind v4. Shared across all platforms.
- **`core/rust-lib/`** — Shared Rust app logic (clipboard watcher, hotkey, paste, db, snippets, tray). All cross-platform.
- **`win/src-tauri/`** and **`macos/src-tauri/`** — Thin per-OS bundle shells. They own `tauri.conf.json`, icons, capabilities, and a 5-line `main.rs` that calls `inspector_rust_core::run(generate_context!())`.

When adding a feature, **prefer adding it to `core/`**. Per-OS bundles are only for things that genuinely differ (entitlements, installer config, icons).

## Workflow

1. **Branch off `main`**. Branch naming is informal — `fix/foo`, `feat/bar`, `docs/baz` all fine.
2. **Make focused commits**. One logical change per commit; `fix:`, `feat:`, `docs:`, `chore:`, `refactor:` prefixes preferred (Conventional Commits).
3. **Run the local check bundle before pushing**:
   ```bash
   bash scripts/check.sh   # cargo clippy + tsc + eslint
   cargo test --workspace  # 192 Rust tests
   pnpm test               # 104 frontend tests
   ```
4. **Open a PR against `main`**. CI will re-run all of the above.

## Code style

### Rust

- `cargo fmt` (default settings) — enforced by clippy.
- No `unwrap()` / `expect()` outside of test code unless the failure is genuinely impossible. Use `anyhow::Result` + `?` and let errors bubble up.
- Structured logging via `tracing` (`tracing::warn!`, `tracing::debug!`, etc.) — never `println!`.
- Never panic in IPC commands. Return `Result<T, String>` and convert errors with `.map_err(map_err)` (see [`commands.rs`](./core/rust-lib/src/commands.rs)).

### TypeScript

- `pnpm format` runs Prettier; `pnpm lint` runs ESLint (incl. `react-hooks/recommended`).
- `strict: true` in `tsconfig.json` — no `any`, no implicit any.
- Prefer named exports. Default exports only for React components used by the entrypoint.
- Tailwind utility-first; CSS variables for theming (see [`core/frontend/src/styles.css`](./core/frontend/src/styles.css) and the `@theme` block).

### Tests

- Rust unit tests live alongside the module they cover (`#[cfg(test)] mod tests`). Use `Connection::open_in_memory()` so tests don't touch real SQLite files.
- Frontend tests use [vitest](https://vitest.dev) + [happy-dom](https://github.com/capricorn86/happy-dom) + [@testing-library/react](https://testing-library.com/docs/react-testing-library/intro). One `*.test.ts(x)` file per source file when worth testing.

## Adding a new IPC command

1. Add the function to [`core/rust-lib/src/commands.rs`](./core/rust-lib/src/commands.rs) with `#[tauri::command]`.
2. Register it in the `invoke_handler!` macro in [`core/rust-lib/src/lib.rs`](./core/rust-lib/src/lib.rs).
3. Add a typed wrapper in [`core/frontend/src/lib/ipc.ts`](./core/frontend/src/lib/ipc.ts).
4. Add a unit test for the Rust function (in-memory DB) and, if it's user-facing, mention it in `README.md`.

## Adding a new platform shell (Linux, etc.)

1. Create `linux/` (or whatever) at the repo root, mirroring the structure of [`win/`](./win) and [`macos/`](./macos).
2. Add `linux/src-tauri/Cargo.toml` (bin crate that depends on `inspector-rust-core` via path).
3. Add `linux/src-tauri/src/main.rs` (5-line entrypoint).
4. Add `linux/src-tauri/tauri.conf.json` with `frontendDist: "../../core/frontend/dist"`.
5. Add `linux/package.json` with the Tauri CLI as devDep + `dev`/`build` proxy scripts.
6. Add to **both** workspace files: `Cargo.toml` (`members = [..., "linux/src-tauri"]`) and `pnpm-workspace.yaml` (`packages: [..., "linux"]`).
7. Add `dev:linux` and `build:linux` scripts to the root `package.json`.
8. Add a job to [`release.yml`](./.github/workflows/release.yml) that builds and uploads the Linux artifacts.
9. Write `linux/README.md`.

## Releasing

See [`docs/RELEASING.md`](./docs/RELEASING.md) for the full release procedure.

## Reporting bugs

Open an issue at <https://github.com/pepperonas/inspector-rust/issues>. Please include:

- Inspector Rust version (tray menu → "About" — TODO, for now check `~/Library/Application Support/InspectorRust/` mod times or the running binary path).
- OS + version (e.g. macOS 14.5, Windows 11 23H2).
- Steps to reproduce.
- Whether the popup hotkey opens the popup at all (rules out half the codebase).

## Code of conduct

Be kind, be constructive. We don't have a formal CoC; the [Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/) is a good default.
