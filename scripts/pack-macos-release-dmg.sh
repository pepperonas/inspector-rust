#!/usr/bin/env bash
#
# pack-macos-release-dmg.sh — seal + repack the GitHub-release macOS DMG.
#
# WHY: `tauri build` leaves the .app with only a *linker* ad-hoc signature
# (flags=adhoc,linker-signed; Sealed Resources=none). Gatekeeper treats that
# as an *invalid* signature and shows the dreaded:
#
#   "InspectorRust.app is damaged and can't be opened. You should move it to
#    the Trash."
#
# A proper `codesign --force --deep --sign -` seals Info.plist + Resources so
# `codesign --verify` passes. Without Apple Developer ID + notarization the
# app is still "unidentified developer" (right-click → Open), but it is no
# longer reported as *damaged*.
#
# Usage (CI, after `tauri build --target <triple>`):
#   bash scripts/pack-macos-release-dmg.sh aarch64-apple-darwin
#   bash scripts/pack-macos-release-dmg.sh x86_64-apple-darwin
#
# Finds the .app under target/<triple>/release/bundle/macos/, resigns it,
# injects the TCC usage strings, and rebuilds the DMG in
# target/<triple>/release/bundle/dmg/.
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_ID="io.celox.inspector-rust"
APP_NAME="InspectorRust.app"
ENTITLEMENTS="${REPO_ROOT}/macos/src-tauri/entitlements.plist"
TRIPLE="${1:?usage: $0 <rust-target-triple>}"

APP="${REPO_ROOT}/target/${TRIPLE}/release/bundle/macos/${APP_NAME}"
DMG_DIR="${REPO_ROOT}/target/${TRIPLE}/release/bundle/dmg"
VERSION="$(
  python3 -c 'import json; print(json.load(open("macos/src-tauri/tauri.conf.json"))["version"])' \
    2>/dev/null \
  || sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
       "${REPO_ROOT}/macos/src-tauri/tauri.conf.json" | head -1
)"
# Arch suffix matches Tauri's naming (aarch64 / x64).
case "${TRIPLE}" in
  aarch64-*) ARCH_TAG="aarch64" ;;
  x86_64-*)  ARCH_TAG="x64" ;;
  *)         ARCH_TAG="${TRIPLE%%-*}" ;;
esac
DMG_OUT="${DMG_DIR}/InspectorRust_${VERSION}_${ARCH_TAG}.dmg"

if [[ ! -d "${APP}" ]]; then
  echo "✘ app bundle missing: ${APP}" >&2
  exit 1
fi

echo "▸ Sealing ${APP} (ad-hoc deep sign)…"

# Same Info.plist keys install-macos.sh injects — needed for Automation /
# Microphone TCC prompts. Must happen BEFORE codesign so they're sealed.
INFO_PLIST="${APP}/Contents/Info.plist"
if [[ -f "${INFO_PLIST}" ]]; then
  plutil -replace NSAppleEventsUsageDescription -string \
    'Inspector Rust uses Apple Events to read your Finder selection so the popup can run actions (resize, OCR, …) on the files you have selected.' \
    "${INFO_PLIST}"
  plutil -replace NSMicrophoneUsageDescription -string \
    'Inspector Rust uses the microphone for live BPM detection — type "bpm" in the popup and press Enter to estimate the tempo of music playing nearby.' \
    "${INFO_PLIST}"
fi

# Ad-hoc (no Developer ID in CI). Do NOT pass `--options runtime` — hardened
# runtime requires a real signing identity and rejects `-`.
SIGN_ARGS=(--force --deep --sign - --identifier "${BUNDLE_ID}")
if [[ -f "${ENTITLEMENTS}" ]]; then
  SIGN_ARGS+=(--entitlements "${ENTITLEMENTS}")
fi
codesign "${SIGN_ARGS[@]}" "${APP}"

echo "▸ Verifying signature…"
if ! codesign --verify --deep --strict "${APP}"; then
  echo "✘ codesign --verify failed — refusing to ship a broken DMG" >&2
  codesign -dv --verbose=4 "${APP}" 2>&1 | sed 's/^/  /' || true
  exit 1
fi
codesign -dv "${APP}" 2>&1 | sed 's/^/  /'

# Gatekeeper helper dropped next to the .app inside the DMG — unsigned
# downloads still need one click past "unidentified developer"; this clears
# the quarantine bit if right-click → Open isn't enough.
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/ir-dmg.XXXXXX")"
cleanup() { rm -rf "${STAGE}"; }
trap cleanup EXIT

cp -R "${APP}" "${STAGE}/${APP_NAME}"
ln -s /Applications "${STAGE}/Applications"

cat > "${STAGE}/Fix Gatekeeper.command" <<'EOF'
#!/bin/bash
# Clears the download quarantine so Gatekeeper stops blocking the app.
# Double-click this AFTER dragging InspectorRust.app to Applications.
set -euo pipefail
APP="/Applications/InspectorRust.app"
if [[ ! -d "${APP}" ]]; then
  osascript -e 'display alert "InspectorRust.app not found in /Applications" message "Drag the app to Applications first, then run this again." as critical'
  exit 1
fi
xattr -cr "${APP}" 2>/dev/null || true
open "${APP}"
osascript -e 'display notification "Quarantine cleared — launching Inspector Rust." with title "Inspector Rust"'
EOF
chmod +x "${STAGE}/Fix Gatekeeper.command"

# Plain-text rescue note. The `.command` helper above inherits the DMG's
# quarantine bit too, so double-clicking IT is *also* refused by Gatekeeper
# ("no usable signature") — the very users who need it are the ones who
# can't run it. A .txt has no such gate: TextEdit opens a quarantined text
# file without a prompt. The leading "!" makes Finder sort it first, so the
# instructions are the first thing visible in the mounted volume.
cat > "${STAGE}/! READ ME FIRST.txt" <<'EOF'
Inspector Rust — macOS first launch
===================================

If macOS says:

    "InspectorRust.app is damaged and can't be opened.
     You should move it to the Trash."

...the download is NOT damaged and the app is NOT broken.

This app is ad-hoc signed but not notarized by Apple (notarization
requires a paid Apple Developer account). macOS flags every such app
downloaded from the internet with a "quarantine" marker, and shows that
misleading "damaged" wording instead of the usual "unidentified
developer" prompt. Right-click -> Open does NOT clear it.

FIX — two steps
---------------

1. Drag InspectorRust.app into the Applications folder.

2. Open Terminal (Cmd+Space, type "Terminal") and run this one line:

       xattr -dr com.apple.quarantine /Applications/InspectorRust.app

   Then launch the app normally. You only ever do this once.

The bundled "Fix Gatekeeper.command" does exactly the same thing, but
macOS may refuse to run it for the same quarantine reason - if it is
blocked, use the Terminal line above.

Verifying the download (optional)
---------------------------------

The app carries a valid, sealed ad-hoc signature. You can confirm the
bundle is intact:

    codesign --verify --deep --strict /Applications/InspectorRust.app

Silence means the signature is valid and nothing is corrupted.
EOF

# Volume icon (best-effort) — Tauri ships one when the dmg step ran.
if [[ -f "${APP}/Contents/Resources/icon.icns" ]]; then
  cp "${APP}/Contents/Resources/icon.icns" "${STAGE}/.VolumeIcon.icns" 2>/dev/null || true
fi

mkdir -p "${DMG_DIR}"
# Drop any unsigned DMG tauri produced so we don't upload the broken one.
rm -f "${DMG_DIR}"/*.dmg

echo "▸ Building ${DMG_OUT}…"
# UDZO = zlib-compressed read-only — same format tauri's bundle_dmg uses.
hdiutil create \
  -volname "InspectorRust" \
  -srcfolder "${STAGE}" \
  -ov \
  -format UDZO \
  -imagekey zlib-level=9 \
  "${DMG_OUT}"

echo "▸ Done: $(ls -lh "${DMG_OUT}" | awk '{print $5, $9}')"
# Final sanity: mount, verify the sealed app inside the new DMG.
MNT="$(hdiutil attach "${DMG_OUT}" -nobrowse -readonly | awk 'END{print $NF}')"
if ! codesign --verify --deep --strict "${MNT}/${APP_NAME}"; then
  hdiutil detach "${MNT}" >/dev/null || true
  echo "✘ DMG contains an unverifiable app — abort" >&2
  exit 1
fi
hdiutil detach "${MNT}" >/dev/null
echo "▸ DMG self-check OK"
