# Cleaning (`clean`)

`clean` in the search bar opens an interactive picker in the preview column: a
read-only dry-run scan, rendered as **directories grouped under their category**
(largest first), each tickable. Nothing is deleted until you arm and confirm
(Enter twice). Implementation: `core/rust-lib/src/cleaner.rs` +
`core/frontend/src/components/CleanPanel.tsx` (selection math:
`core/frontend/src/lib/clean-select.ts`).

**Background jobs (v0.101.1).** Scan and execute run entirely in the Rust
backend and **survive Esc / overlay-hide** (same pattern as `shazam`). Closing
the panel only drops the UI; the walk/delete finishes, emits `clean-done`, and —
when the overlay is closed — App shows a status toast (`Scan ready` /
`Cleaned`). Reopening `clean` reconnects via `cleaner_status` (in-flight) or
reuses the pending scan view so you don't re-walk the disk.

## Safety guarantees

These are the invariants the module exists to uphold. Everything else is detail.

1. **Strict allowlist, never a blocklist.** Deletion can only ever happen inside
   the hard-coded roots of `cleaner::categories()`. Documents, Desktop, Pictures,
   source code — unreachable by construction.
2. **Canonicalise + containment check before every delete.** A file is removed
   only if its canonical path is genuinely under an allowed root. Checked at
   scan time *and again* at execute time (TOCTOU-resistant).
3. **Symlinks are never followed.** The walker `lstat`s and skips them, so a
   symlink planted inside a cache dir can't smuggle an outside path into the plan.
4. **Dry-run first, and only what you ticked.** `scan` is read-only. The
   file-granular plan stays in the backend (`PlanStore`); the UI gets aggregated
   directory rows, and `execute` deletes only the files under the rows you ticked
   — **file by file**, never `remove_dir_all`, so nothing that appeared *after*
   the scan is swept up.
5. **Levels + age filter.** `Safe` (default) → `Standard` → `Aggressive`; each
   category declares the level at which it becomes eligible and whether it's on
   by default. Only files older than `min_age_days` are touched.
6. **External tools are never required and never elevated.** The command-based
   categories run only if their binary is on PATH, never with `sudo`, and a
   failure is collected as an error instead of aborting the run.
7. **Our own data is never in an allowlist root** — `history.db` / `.dbkey` live
   in the app-support dir, which no category owns.

## Categories

### Safe (default level)

| Key | What |
|---|---|
| `app_cache` | Inspector Rust's own cache dir |
| `os_temp` | OS temp dir |

### Standard

| Key | What |
|---|---|
| `browser_cache` | Browser caches (never cookies/logins) |
| `other_caches` | The whole user cache dir (`~/Library/Caches` / `~/.cache`), with the browser + app roots carved out. This is where JetBrains caches, Yarn, pip, CocoaPods and the Homebrew cache already live |
| `editor_caches` | VS Code / Cursor / VSCodium index, GPU + renderer caches (in the app-data dir, invisible to `other_caches`) |
| `logs` | Application logs |
| `installers` | Old `dmg`/`pkg`/`iso`/… in Downloads (age filter applies) — **off by default** |
| `dupes` | Byte-identical duplicates in Downloads, oldest copy always kept — **off by default** |
| `docker` | Docker **build cache** only, via `docker builder prune`. Images and volumes are never touched — they live inside one VM disk file, where file-level deletion would destroy everything — **off by default** |
| `jetbrains_orphans` | Support + log dirs of JetBrains IDE versions that are gone (macOS) |
| `brew_cleanup` | Homebrew's outdated downloads, via `brew cleanup` (macOS) |
| `pnpm_store` | Orphaned packages in the pnpm store, via `pnpm store prune` |
| `stale_node_modules` | `node_modules` of projects untouched for `stale_days` — **off by default** |
| `stale_rust_target` | Cargo `target/` of projects untouched for `stale_days` — **off by default** |

### Aggressive (opt-in, each also off by default)

| Key | What |
|---|---|
| `dev_caches` | Global dev-tool caches: npm `_cacache`, pnpm store, Gradle caches/daemon/wrapper, `.m2/repository`, Cargo registry cache + src + git, rustup, uv, Android |
| `xcode_caches` | DerivedData, CoreSimulator caches, iOS DeviceSupport, XCTestDevices |
| `xcode_archives` | Xcode archives — **these contain the dSYMs**; without them you cannot symbolicate a crash report from a shipped build |
| `simctl_unavailable` | Simulators macOS marks unavailable, via `xcrun simctl delete unavailable` |
| `trash` | Trash items older than the age filter |

## The developer targets in detail (v0.84.264, macOS-first)

### Stale build artifacts

The only categories whose allowlist is **derived** rather than hard-coded: their
roots are the concrete `node_modules` / `target` directories found under the
user's **dev roots** (Settings → Cleaning; default `~/claude`, `~/cursor`,
`~/dev`; empty = no project scanning at all).

Two gates decide whether an artifact is listed:

- **Plausibility.** The artifact is only ever reported when the matching manifest
  (`package.json` / `Cargo.toml`) sits **right next to it**. A bare `node_modules`
  with no manifest beside it is left alone — it might be anything.
- **Staleness.** The project's *last human activity* — the newest mtime among the
  manifest, `src`/`lib`, `.git/HEAD`, `README.md` and the project dir itself —
  must be at least `stale_days` (default 90) old. Deliberately **not** the
  artifact's own mtime: a background `cargo build` would otherwise keep a dead
  project looking alive.

The per-file age filter is bypassed for these categories (`ignore_age`):
staleness was already decided per *project*, and a dependency install that never
led anywhere leaves fresh files inside an artifact we still want to reclaim.
Symlinked artifacts are never reported; the walk never descends into
`node_modules` / `target` / `.git` / `Pods` / hidden dirs, and stops at depth 5.

### JetBrains orphans

A support/log dir is an orphan when either its product isn't installed at all (no
matching app bundle in `/Applications` or `~/Applications`), **or** a newer
version dir of the same product exists — the newest is always kept, because
that's the one the installed IDE uses. Version comparison is numeric per segment,
so `2023.10` correctly outranks `2023.2`. A dir with no parseable version is never
touched.

### Command-based categories

`docker` · `brew_cleanup` · `pnpm_store` · `simctl_unavailable` have **no file
roots** — they never touch the file allowlist. They are previewed with the tool's
own dry-run and, if ticked, executed by running the tool's reclaim command:

| Category | Preview | Execute |
|---|---|---|
| `docker` | `docker system df` | `docker builder prune -f` |
| `brew_cleanup` | `brew cleanup --dry-run` | `brew cleanup` |
| `pnpm_store` | *(size unknown — pnpm reports no dry-run size, and it's shown as such rather than inventing a number)* | `pnpm store prune` |
| `simctl_unavailable` | `xcrun simctl list devices` + the size of the matching device dirs | `xcrun simctl delete unavailable` |

A missing binary means the category simply contributes nothing — never an error.

## Settings

Settings → **Cleaning**: level, age threshold, per-category checkboxes, and — for
the stale-artifact scanner — the **dev roots** (one path per line) and the
**stale threshold** in days.

## Tests

`cleaner.rs`'s test module drives the pure core (`scan_roots`, `execute_plan`,
`is_contained`, `find_stale_artifacts`, `jetbrains_orphans`, `aggregate_dirs`,
`filter_by_selection`, the tool-output parsers) against **temp fixtures only** —
no test ever touches a real user path. Covered: containment, symlink-escape
rejection, outside-allowlist rejection, the age filter, per-category exclusions
at scan *and* execute, staleness (fresh project kept, dead project found, recent
source edits keep it alive), the plausibility gate, symlinked artifacts,
`ignore_age`, JetBrains version ranking, directory aggregation, and
selection → plan consistency.
