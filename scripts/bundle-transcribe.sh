#!/usr/bin/env bash
# bundle-transcribe.sh — put the `ir-transcribe` live-transcription helper
# into an app bundle, next to the main executable (before signing, so the
# bundle's signature covers it).
#
#   bash scripts/bundle-transcribe.sh <path/to/InspectorRust.app> [rust-target-triple]
#
# Apple Silicon only: whisper.cpp runs on Metal there. For any other triple
# this is a no-op and the app reports transcription as unavailable.
set -euo pipefail

APP="${1:?usage: bundle-transcribe.sh <app> [triple]}"
TRIPLE="${2:-}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

arch="${TRIPLE%%-*}"
[[ -z "${TRIPLE}" ]] && arch="$(uname -m)"
if [[ "${arch}" != "aarch64" && "${arch}" != "arm64" ]]; then
  echo "▸ ir-transcribe: skipped for ${TRIPLE:-$(uname -m)} (Apple Silicon only)"
  exit 0
fi

if [[ -n "${TRIPLE}" ]]; then
  env -u CARGO_TARGET_DIR cargo build --release --target "${TRIPLE}" \
    -p transcribe-helper --bin ir-transcribe --manifest-path "${REPO_ROOT}/Cargo.toml"
  BIN="${REPO_ROOT}/target/${TRIPLE}/release/ir-transcribe"
else
  env -u CARGO_TARGET_DIR cargo build --release \
    -p transcribe-helper --bin ir-transcribe --manifest-path "${REPO_ROOT}/Cargo.toml"
  BIN="${REPO_ROOT}/target/release/ir-transcribe"
fi

[[ -x "${BIN}" ]] || { echo "✘ bundle-transcribe: ${BIN} missing" >&2; exit 1; }
# whisper.cpp is statically linked; anything pointing at Homebrew or the
# build tree would break on another machine.
if otool -L "${BIN}" | tail -n +2 | grep -Eq "/opt/homebrew|/usr/local|${REPO_ROOT}"; then
  echo "✘ bundle-transcribe: ${BIN} links a non-system library:" >&2
  otool -L "${BIN}" >&2
  exit 1
fi
cp "${BIN}" "${APP}/Contents/MacOS/ir-transcribe"
xattr -c "${APP}/Contents/MacOS/ir-transcribe" 2>/dev/null || true
mkdir -p "${APP}/Contents/Resources/THIRD-PARTY/transcribe"
cp "${REPO_ROOT}/transcribe-core/THIRD-PARTY.md" "${APP}/Contents/Resources/THIRD-PARTY/transcribe/README.md"
echo "▸ Bundled the ir-transcribe helper"
