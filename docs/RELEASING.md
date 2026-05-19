# Releasing Inspector Rust

Inspector Rust releases are published automatically by [GitHub Actions](../.github/workflows/release.yml) when a `v*` tag is pushed. This document describes the full release flow end to end.

## TL;DR

```bash
# 1. Bump version everywhere (see "Files to bump")
# 2. Update CHANGELOG.md with the new version
# 3. Commit
git commit -am "chore(release): v0.x.y"

# 4. Verify the workspace is healthy
bash scripts/check.sh
cargo test --workspace
pnpm test

# 5. Tag and push
git tag v0.x.y
git push origin main
git push origin v0.x.y

# 6. Watch the release workflow
gh run watch
```

The workflow builds Windows + macOS bundles and attaches them to the GitHub Release.

## Versioning policy

Inspector Rust follows [Semantic Versioning](https://semver.org). Until 1.0.0:

- **`0.x.0`** — new features, possibly with breaking changes.
- **`0.x.y` (y > 0)** — bug fixes, compatible additions only.

Pre-releases (`v0.x.0-beta.1` etc.) are allowed; the release workflow treats them the same as stable tags but you can mark them as "Pre-release" in the GitHub UI afterwards.

## Files to bump

When cutting `v0.x.y`, replace the previous version in **all** of these:

| File | Field |
|------|-------|
| `Cargo.toml` | `[workspace.package].version` |
| `package.json` | `version` |
| `core/frontend/package.json` | `version` |
| `win/package.json` | `version` |
| `macos/package.json` | `version` |
| `win/src-tauri/tauri.conf.json` | `version` |
| `macos/src-tauri/tauri.conf.json` | `version` |
| `Cargo.lock` | three `inspector-rust-{core,win,macos}` entries — `cargo` regenerates this on next build, but bumping it in the same commit avoids a phantom diff in the release commit |

A grep-and-sanity check:

```bash
grep -rn '"version"\|^version' \
  Cargo.toml package.json \
  core/frontend/package.json \
  win/package.json macos/package.json \
  win/src-tauri/tauri.conf.json macos/src-tauri/tauri.conf.json
grep -n -A1 'name = "inspector-rust' Cargo.lock
```

All seven manifest lines plus the three `Cargo.lock` workspace-crate entries should show the new version.

### One-shot bump script

For convenience, the version can be bumped in one shot with `perl`:

```bash
OLD=0.x.y ; NEW=0.x.z
perl -i -pe "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" \
  package.json macos/package.json core/frontend/package.json win/package.json \
  macos/src-tauri/tauri.conf.json win/src-tauri/tauri.conf.json
perl -i -pe "s/^version = \"$OLD\"\$/version = \"$NEW\"/" Cargo.toml
perl -i -0pe "s/(name = \"inspector-rust-(?:core|win|macos)\"\\nversion = \")$OLD(\")/\$1 . \"$NEW\" . \$2/ge" Cargo.lock
```

## Pre-flight checks

```bash
bash scripts/check.sh        # cargo clippy + tsc + eslint
cargo test --workspace       # 110 Rust unit tests
pnpm test                    # 86 frontend tests (vitest)
```

If any fail, do **not** tag. Fix on `main` first.

## Tagging

```bash
git tag v0.x.y               # annotated optional, but consistent with prior tags
git push origin v0.x.y
```

The push triggers [`release.yml`](../.github/workflows/release.yml). Two parallel jobs:

| Job | Runs on | Builds | Uploads |
|-----|---------|--------|---------|
| `build-windows` | `windows-latest` | `pnpm build:win` | `target/release/inspector-rust.exe`, `target/release/bundle/msi/*.msi` |
| `build-macos`   | `macos-latest`   | `pnpm build:macos` | `target/release/bundle/dmg/*.dmg` |

Both write to the same release using `softprops/action-gh-release@v2`. The Windows job creates the release and generates release notes from commit history; the macOS job attaches its DMG to that same release.

## Watching the workflow

```bash
gh run list --workflow release.yml --branch main --limit 3
gh run watch <RUN_ID>           # blocks until done
gh release view v0.x.y          # confirm assets are attached
```

Expected assets after a successful run:

- `inspector-rust.exe` (Windows standalone executable, ~14 MB)
- `inspector-rust_<version>_x64_en-US.msi` (Windows installer, ~5 MB)
- `inspector-rust_<version>_aarch64.dmg` (macOS Apple Silicon DMG, ~5 MB)

## If a build fails

Re-run the failing job from the GitHub UI (**Actions → Release → Re-run failed jobs**) — most failures are transient runner issues. If it keeps failing:

- **Windows MSI step fails** with WiX error → check that `tauri.conf.json` has valid `bundle.windows.wix.language`.
- **macOS DMG step fails** in `bundle_dmg.sh` → known flaky on busy GitHub runners. Workaround: change the macOS step from `pnpm build:macos` to `cd macos && pnpm tauri build --bundles app`, then upload the `.app` zipped (CI doesn't natively zip the bundle; add a `zip -r` step before upload).
- **`pnpm install` fails** with `ERR_PNPM_OUTDATED_LOCKFILE` → you forgot to commit `pnpm-lock.yaml` after changing a `package.json`. Run `pnpm install` locally and commit the lockfile.

## Manual rollout (last resort)

If GitHub Actions is unavailable, you can build and upload manually:

```bash
# Windows (must be a Windows host)
pnpm install
pnpm build:win
gh release upload v0.x.y \
  target/release/inspector-rust.exe \
  target/release/bundle/msi/inspector-rust_*.msi

# macOS (must be a macOS host)
pnpm install
pnpm build:macos
gh release upload v0.x.y \
  target/release/bundle/dmg/inspector-rust_*.dmg
```

Or seed the release without binaries:

```bash
gh release create v0.x.y --generate-notes
```

…then upload assets later.

## Post-release

1. Verify the release page lists the expected assets (above).
2. Update [`README.md`](../README.md) badges if you embed a version (the version badge currently links to releases generically — no manual edit needed).
3. Optional: post to whatever channel announces Inspector Rust (Slack, etc.).
4. Open issues for any follow-up work that surfaced during the release prep.

## Hotfix flow

For a fix that needs to ship immediately on top of an already-released version:

```bash
# from a clean main
git switch -c hotfix/v0.x.y main
# … fix, test, commit …
git switch main
git merge --no-ff hotfix/v0.x.y
# bump version (patch)
git commit -am "chore(release): v0.x.(y+1)"
git tag v0.x.(y+1)
git push origin main v0.x.(y+1)
```
