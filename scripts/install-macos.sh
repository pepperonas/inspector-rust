#!/usr/bin/env bash
#
# install-macos.sh — build, re-sign, and install ClipSnap into /Applications.
#
# WHY THIS SCRIPT EXISTS:
#   ClipSnap is unsigned (no Apple Developer ID). On macOS Tahoe (26+) the
#   TCC database binds Accessibility grants to the tuple (bundle id, cdhash).
#   Any change to the cdhash invalidates the prior grant. Plain `tauri build`
#   leaves the binary linker-signed with a *random* identifier (e.g.
#   `clipsnap-c64f925d…`); calling `codesign --force` adds a fresh CMS
#   timestamp on every invocation, which produces a new cdhash even when
#   the underlying binary hasn't changed.
#
#   So the sequence "rebuild → install → grant → rebuild → install" used to
#   silently invalidate the grant on every iteration. This script:
#
#   1. Hashes the *source tree* (.rs / .ts / .tsx / .json / Cargo.lock /
#      pnpm-lock.yaml / entitlements / capabilities) into a single SHA-256.
#   2. Compares to a stamp file written into the previously-installed
#      bundle (Contents/Resources/.clipsnap-source-hash). Match → skip the
#      `tauri build` *and* the re-sign entirely; just re-launch the existing
#      install. cdhash stays stable, the TCC grant survives.
#   3. Mismatch → run `tauri build`, copy into /Applications, re-sign with
#      the stable bundle identifier `io.celox.clipsnap` + entitlements +
#      Hardened Runtime, write the new source-hash stamp, and launch.
#
#   We hash the *source* (not the built binary) because Rust release builds
#   aren't byte-reproducible — even with `codegen-units=1` + `lto=true`,
#   two consecutive `cargo build --release` runs of the same source produce
#   slightly different bytes (paths, link-time decisions). Comparing source
#   inputs is the reliable signal.
#
#   Net effect: rebuilding without source changes never asks the user to
#   re-grant Accessibility. Real source changes still produce a new cdhash
#   and re-grant is needed (the in-app banner handles that gracefully).
#
# USAGE:
#   bash scripts/install-macos.sh         # build + install (idempotent) + launch
#   bash scripts/install-macos.sh --reset # …also tccutil-reset stale grants
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_ID="io.celox.clipsnap"
APP_NAME="ClipSnap.app"
BUILD_OUT="${REPO_ROOT}/target/release/bundle/macos/${APP_NAME}"
INSTALL_PATH="/Applications/${APP_NAME}"
ENTITLEMENTS="${REPO_ROOT}/macos/src-tauri/entitlements.plist"

DO_RESET=0
for arg in "$@"; do
  case "$arg" in
    --reset) DO_RESET=1 ;;
    -h|--help)
      sed -n '2,/^set -euo pipefail/p' "$0" | sed 's/^# \{0,1\}//' | head -n -1
      exit 0
      ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

# ── helpers ─────────────────────────────────────────────────────────────────

bin_sha256() {
  # SHA-256 of the actual Mach-O — not the .app bundle wrapper.
  # NB: bash 3.2 (the macOS default) propagates non-zero exits from
  # command substitutions in `set -e` mode, so every helper here ends
  # with `return 0` to keep the caller's `var=$(helper)` from blowing up
  # the whole script when the helper just had nothing to print.
  local app="$1"
  local exe="${app}/Contents/MacOS/clipsnap"
  if [[ -f "${exe}" ]]; then
    shasum -a 256 "${exe}" | cut -d' ' -f1
  fi
  return 0
}

# SHA-256 over every input that meaningfully affects the built binary.
# Rust release builds aren't byte-reproducible (paths, timestamps, LLVM
# non-determinism), so comparing the *output* across rebuilds always
# differs. Comparing the *inputs* gives us a reliable "did anything that
# matters actually change?" signal — and lets us skip the build itself,
# not just re-signing, when nothing did.
source_sha256() {
  (
    cd "${REPO_ROOT}"
    {
      find core/rust-lib/src core/rust-lib/Cargo.toml \
           core/frontend/src core/frontend/index.html \
           core/frontend/package.json core/frontend/tsconfig.json \
           core/frontend/vite.config.ts \
           macos/src-tauri/src macos/src-tauri/Cargo.toml \
           macos/src-tauri/tauri.conf.json macos/src-tauri/entitlements.plist \
           macos/src-tauri/build.rs macos/src-tauri/capabilities \
           Cargo.toml Cargo.lock pnpm-lock.yaml \
           -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' \
                   -o -name '*.json' -o -name '*.html' -o -name '*.toml' \
                   -o -name '*.lock' -o -name '*.yaml' -o -name '*.css' \
                   -o -name 'entitlements.plist' \) \
           2>/dev/null
    } | LC_ALL=C sort | xargs shasum -a 256 2>/dev/null | shasum -a 256 | cut -d' ' -f1
  )
}

stored_source_hash() {
  local app="$1"
  local f="${app}/Contents/Resources/.clipsnap-source-hash"
  [[ -f "${f}" ]] && cat "${f}"
  return 0
}

write_source_hash() {
  local app="$1"
  local hash="$2"
  echo -n "${hash}" > "${app}/Contents/Resources/.clipsnap-source-hash"
}

cdhash() {
  local app="$1"
  if [[ -d "${app}" ]]; then
    # `--verbose=4` is required to get the `CDHash=...` line; -dv alone
    # only prints `CodeDirectory v=… size=… flags=… hashes=… location=…`.
    codesign -dvvv "${app}" 2>&1 | sed -n 's/^CDHash=//p'
  fi
  return 0
}

current_identifier() {
  local app="$1"
  if [[ -d "${app}" ]]; then
    codesign -dv "${app}" 2>&1 | sed -n 's/^Identifier=//p'
  fi
  return 0
}

kill_running() {
  local pids
  pids=$(pgrep -f "${INSTALL_PATH}/Contents/MacOS/clipsnap" || true)
  if [[ -n "${pids}" ]]; then
    echo "  stopping ClipSnap PIDs: ${pids}"
    kill ${pids} 2>/dev/null || true
    sleep 1
  fi
}

resign_app() {
  local app="$1"
  if [[ -f "${ENTITLEMENTS}" ]]; then
    codesign --force --deep --sign - \
      --identifier "${BUNDLE_ID}" \
      --entitlements "${ENTITLEMENTS}" \
      --options runtime \
      "${app}"
  else
    codesign --force --deep --sign - --identifier "${BUNDLE_ID}" "${app}"
  fi
}

reset_tcc() {
  echo "▸ Resetting stale TCC grants for ${BUNDLE_ID}…"
  tccutil reset Accessibility "${BUNDLE_ID}" 2>/dev/null || true
  tccutil reset PostEvent     "${BUNDLE_ID}" 2>/dev/null || true
  tccutil reset ScreenCapture "${BUNDLE_ID}" 2>/dev/null || true
  echo "  → next launch re-prompts for Accessibility + Screen Recording."
}

# ── 1) source hash — decide *before* building whether we even need to ──────

echo "▸ Hashing source tree…"
NEW_SRC_HASH="$(source_sha256)"
OLD_SRC_HASH="$(stored_source_hash "${INSTALL_PATH}")"
INSTALLED_ID="$(current_identifier "${INSTALL_PATH}")"

UNCHANGED=0
if [[ -n "${NEW_SRC_HASH}" \
   && -n "${OLD_SRC_HASH}" \
   && "${NEW_SRC_HASH}" == "${OLD_SRC_HASH}" \
   && "${INSTALLED_ID}" == "${BUNDLE_ID}" \
   && -d "${INSTALL_PATH}" ]]; then
  UNCHANGED=1
fi

if [[ "${UNCHANGED}" -eq 1 ]]; then
  echo "▸ Source unchanged (sha256: ${NEW_SRC_HASH:0:12}…) — skipping build"
  echo "  Identifier: ${INSTALLED_ID}"
  echo "  cdhash:     $(cdhash "${INSTALL_PATH}")"
  echo "  → existing TCC grant for this cdhash will survive"

  kill_running
  if [[ "${DO_RESET}" -eq 1 ]]; then
    reset_tcc
  fi
  echo "▸ Launching…"
  open "${INSTALL_PATH}"
  echo
  echo "✓ Re-launched $(defaults read "${INSTALL_PATH}/Contents/Info.plist" CFBundleShortVersionString) at ${INSTALL_PATH}"
  exit 0
fi

# ── 2) source changed — build ───────────────────────────────────────────────

echo "▸ Source changed (or first install) — full build + install"
[[ -n "${OLD_SRC_HASH}" ]] && echo "  old src sha256: ${OLD_SRC_HASH:0:12}…"
echo "  new src sha256: ${NEW_SRC_HASH:0:12}…"
echo "  ⚠ cdhash WILL change — TCC Accessibility grant must be re-given"
echo "    (the app's Settings panel auto-detects this and walks you"
echo "     through the steps; takes ~30 seconds)"

echo "▸ Building ClipSnap.app (release, --bundles app)…"
cd "${REPO_ROOT}"
pnpm --filter clipsnap-macos tauri build --bundles app

if [[ ! -d "${BUILD_OUT}" ]]; then
  echo "✘ build output missing: ${BUILD_OUT}" >&2
  exit 1
fi

# ── 3) install ──────────────────────────────────────────────────────────────

kill_running

echo "▸ Replacing ${INSTALL_PATH}…"
rm -rf "${INSTALL_PATH}"
cp -R "${BUILD_OUT}" "${INSTALL_PATH}"

# Quarantine attribute: set by `cp` from a different volume; it triggers
# Gatekeeper "are you sure you want to open" and *also* invalidates the
# code signature for some macOS versions.
xattr -dr com.apple.quarantine "${INSTALL_PATH}" 2>/dev/null || true

echo "▸ Re-signing with stable identifier ${BUNDLE_ID}…"
if [[ -f "${ENTITLEMENTS}" ]]; then
  echo "  using entitlements: ${ENTITLEMENTS}"
fi
resign_app "${INSTALL_PATH}"

# Stamp the source hash *after* signing so future runs detect "no source
# change" and skip rebuild entirely.
write_source_hash "${INSTALL_PATH}" "${NEW_SRC_HASH}"

if [[ "${DO_RESET}" -eq 1 ]]; then
  reset_tcc
fi

echo "▸ Verifying signature…"
codesign -dv "${INSTALL_PATH}" 2>&1 | sed 's/^/  /'
echo "  Identifier:        $(current_identifier "${INSTALL_PATH}")"
echo "  cdhash this build: $(cdhash "${INSTALL_PATH}")"
echo "  source hash:       ${NEW_SRC_HASH:0:12}…"

echo "▸ Launching…"
open "${INSTALL_PATH}"

echo
echo "✓ Installed $(defaults read "${INSTALL_PATH}/Contents/Info.plist" CFBundleShortVersionString) at ${INSTALL_PATH}"
echo
echo "If Accessibility access is missing after launch:"
echo "  • Open ClipSnap → Settings tab. The green Restart prompt appears once"
echo "    you toggle ClipSnap on in System Settings → Accessibility."
echo "  • Or: bash scripts/install-macos.sh --reset (wipes stale TCC entries)."
