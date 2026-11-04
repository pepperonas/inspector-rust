#!/usr/bin/env bash
# bundle-record-cli.sh — put the `ir-record` command line (+ MCP server) into
# an app bundle, next to the main executable, BEFORE the app is signed so the
# bundle's signature covers it.
#
#   bash scripts/bundle-record-cli.sh <path/to/InspectorRust.app> [rust-target-triple]
#
# Called by install-macos.sh (no triple = host build) and by
# pack-macos-release-dmg.sh (with the release triple).
#
# Apple Silicon only, like the rest of `record`: for any other triple this is a
# no-op. `ir-record` itself only talks to the app's control socket; it links
# nothing but the system libraries (checked below).
set -euo pipefail

APP="${1:?usage: bundle-record-cli.sh <app> [triple]}"
TRIPLE="${2:-}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_NAME="ir-record"

die() { echo "✘ bundle-record-cli: $*" >&2; exit 1; }

arch="${TRIPLE%%-*}"
[[ -z "${TRIPLE}" ]] && arch="$(uname -m)"
if [[ "${arch}" != "aarch64" && "${arch}" != "arm64" ]]; then
  echo "▸ ${BIN_NAME}: skipped for ${TRIPLE:-$(uname -m)} (Apple Silicon only)"
  exit 0
fi
[[ -d "${APP}/Contents/MacOS" ]] || die "not an app bundle: ${APP}"

if [[ -n "${TRIPLE}" ]]; then
  env -u CARGO_TARGET_DIR cargo build --release --target "${TRIPLE}" \
    -p ir-record-cli --bin "${BIN_NAME}" --manifest-path "${REPO_ROOT}/Cargo.toml"
  BIN="${REPO_ROOT}/target/${TRIPLE}/release/${BIN_NAME}"
else
  env -u CARGO_TARGET_DIR cargo build --release \
    -p ir-record-cli --bin "${BIN_NAME}" --manifest-path "${REPO_ROOT}/Cargo.toml"
  BIN="${REPO_ROOT}/target/release/${BIN_NAME}"
fi

[[ -x "${BIN}" ]] || die "${BIN} missing"
if otool -L "${BIN}" | tail -n +2 | grep -Eq "/opt/homebrew|/usr/local|${REPO_ROOT}"; then
  echo "✘ bundle-record-cli: ${BIN} links a non-system library:" >&2
  otool -L "${BIN}" >&2
  exit 1
fi
cp "${BIN}" "${APP}/Contents/MacOS/${BIN_NAME}"
xattr -c "${APP}/Contents/MacOS/${BIN_NAME}" 2>/dev/null || true
echo "▸ Bundled ${BIN_NAME} (command line + MCP server)"
