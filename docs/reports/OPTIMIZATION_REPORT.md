# Optimization Report

> Phase E. Generated 2026-06-06 · base version 0.61.0. Analysis only — the
> safe profile-level quick-wins the plan anticipated are **already applied**
> (see "Already done"), so this report recommends rather than re-applies them.

## Quick wins first

| Optimisation | Category | Est. effect | Effort | Risk | Status |
|---|---|---|---|---|---|
| `strip = true` (symbols too, not just debuginfo) | Size | small (−few %) | S | Low* | Candidate — see note |
| Lazy-load the 4 games (`React.lazy`) | Bundle | medium (defer ~game code) | S | Low | Recommended |
| `ort` behind an optional `cutout` cargo feature | Size | **large (~40 MB)** | M | Med | Recommended |
| `ort` `load-dynamic` instead of `download-binaries` | Size | large | M | Med-High | Evaluate |
| Cache `getImageData` in screenshot-editor blur redraw | Runtime | small | S | Low | Optional |

\* `strip = "debuginfo"` is deliberately chosen in `Cargo.toml` (comment: cdhash
stability for the macOS TCC grant). `strip = true` is also deterministic and
smaller; the choice is conservative. **Not changed** without confirming it
doesn't affect the reproducible-build goal.

## Findings by category

### Size (the dominant lever)
- **ONNX Runtime (`ort`, static via `download-binaries`) is by far the biggest
  contributor** (~40 MB per CLAUDE.md). The README's "~5 MB DMG/MSI" claim
  predates the ML cutout and no longer matches — **measure + reconcile the
  README**. Options, in order of preference:
  1. Put the U²-Net cutout behind an optional cargo feature (`cutout`), default
     on, so size-sensitive builds can drop it. The model file
     (`models/u2netp.onnx`, ~4.5 MB `include_bytes!`) goes behind the same gate.
  2. `ort` `load-dynamic` — ships the runtime as a sidecar `.dylib/.dll`
     instead of static. Smaller binary, but adds a packaging step + a runtime
     load path that needs its own error handling.
- Release profile is **already tuned**: `codegen-units = 1`, full `lto = true`,
  `strip = "debuginfo"`, `opt-level = 3`. `panic = "abort"` would shave more but
  risks Tauri/`catch_unwind` paths — **not recommended** without testing.

### Startup
- `app_launcher::scan()` walks `/Applications`, `~/Applications`,
  `/System/Applications(/Utilities)` on the setup thread (~20–100 ms for
  200–400 bundles). It's already off the main loop; could be made lazy
  (first `list_apps` call) to shave cold-start, but the gain is small.
- First ONNX model load (~150 ms) is already `OnceLock`-cached → paid once.
- `auto_expand::AbbrevTable::from_db` runs at startup + on every snippet CRUD;
  it's an `O(n log n)` sort over the snippet count (tiny) — negligible.

### Runtime
- Clipboard watcher poll, TOTP 1 s refresh, timer 200 ms poll — all modest and
  already documented. No change recommended.
- **Screenshot-editor blur** rebuilds an off-screen canvas + `getImageData`
  on every redraw frame (acknowledged in a code comment). For a session with
  several blur regions this is wasteful; cache the sampled mosaic per blur
  annotation. Low priority (only bites during active editing).
- The new **auto-expansion** monitor does a buffer feed (O(table) suffix scan)
  per keystroke under a mutex — table is small, scan is cheap; fine.

### Memory
- App-icon LRU cache is bounded (cap 100) — good.
- Pin windows each keep a cached PNG copy on disk (not memory) and are deleted
  on close — fine.

### DB
- Indices exist on `abbreviation` and `LOWER(issuer)`; `entries` pruned to
  1000 rows. Consider an occasional `VACUUM` after large deletes (e.g. after
  `clear_history`) to reclaim file size — S effort, low risk, optional.

### Bundle / frontend
- **Code-split the games** (`PongGame`, `SnakeGame`, `SpaceInvadersGame`,
  `BpmDetector`) with `React.lazy` + `Suspense` — they're easter eggs loaded
  by exact triggers, so they don't belong in the initial chunk. Clean S win.
- `lucide-react` is already tree-shaken via named imports.

## Already done (no action)
- Release profile flags (codegen-units/lto/strip/opt-level).
- ONNX session caching (`OnceLock`).
- App-icon bounded LRU; app scan off the main thread.
- 60 fps game loops use `useRef` (no per-frame React re-render).
