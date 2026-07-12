#!/usr/bin/env bash
# update-badges.sh — recompute the README headline metrics (LOC + unit-test
# counts) from the REAL sources and rewrite the badges in README.md +
# README.de.md in place. Idempotent: patterns match whatever numbers are
# currently in the files, so re-running is always safe.
#
#   pnpm badges          # or: bash scripts/update-badges.sh
#
# What it computes:
#   • Lines of code — all workspace Rust (`core/rust-lib/src` + the three
#     2-line platform shells) with the trailing `#[cfg(test)] mod tests`
#     blocks stripped (they are file-final by repo convention), plus all
#     frontend `src/**/*.ts(x)` excluding `*.test.ts(x)` and the
#     auto-generated `openers-data.ts`. node_modules/target/dist never enter
#     the count (only the explicit source dirs are scanned).
#   • Test counts — from the actual runners: the summed `N passed` of every
#     `test result:` line of `cargo test --workspace`, and the
#     `Tests  N passed` summary of `pnpm test` (vitest). The script ABORTS
#     if either suite fails — badges must never advertise a red suite.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "── Counting lines of code…"
RUST_LOC=$(find core/rust-lib/src win/src-tauri/src macos/src-tauri/src linux/src-tauri/src -name '*.rs' -print0 |
  xargs -0 awk '/^#\[cfg\(test\)\]/{skip[FILENAME]=1} {if(!skip[FILENAME]) n++} END{print n}')
FE_LOC=$(find core/frontend/src \( -name '*.ts' -o -name '*.tsx' \) \
  ! -name '*.test.ts' ! -name '*.test.tsx' ! -name 'openers-data.ts' -print0 |
  xargs -0 awk 'END{print NR}')
LOC=$((RUST_LOC + FE_LOC))
LOC_K=$(((LOC + 500) / 1000))
echo "   Rust (sans test mods): ${RUST_LOC} · Frontend (sans tests/generated): ${FE_LOC} → ${LOC} (~${LOC_K}k)"

echo "── Running cargo test --workspace…"
CARGO_OUT=$(cargo test --workspace 2>&1) || { echo "$CARGO_OUT" | tail -30; echo "✗ Rust tests failed — badges NOT updated."; exit 1; }
RUST_TESTS=$(echo "$CARGO_OUT" | awk '/^test result:/{s+=$4} END{print s}')
echo "   Rust: ${RUST_TESTS} passed"

echo "── Running pnpm test (vitest)…"
FE_OUT=$(pnpm test 2>&1) || { echo "$FE_OUT" | tail -30; echo "✗ Frontend tests failed — badges NOT updated."; exit 1; }
FE_TESTS=$(echo "$FE_OUT" | grep -Eo 'Tests[[:space:]]+[0-9]+ passed' | grep -Eo '[0-9]+' | head -1)
echo "   Frontend: ${FE_TESTS} passed"

if [[ -z "$RUST_TESTS" || -z "$FE_TESTS" || "$RUST_TESTS" -eq 0 || "$FE_TESTS" -eq 0 ]]; then
  echo "✗ Could not parse test counts (rust='${RUST_TESTS}' frontend='${FE_TESTS}') — badges NOT updated."
  exit 1
fi
TOTAL=$((RUST_TESTS + FE_TESTS))

echo "── Rewriting badges (LOC ~${LOC_K}k · ${TOTAL} tests = ${RUST_TESTS} Rust + ${FE_TESTS} frontend)…"
for f in README.md README.de.md; do
  # Hero badges (both READMEs share these shield URLs).
  perl -pi -e "s/lines%20of%20code-~\\d+k/lines%20of%20code-~${LOC_K}k/g" "$f"
  perl -pi -e "s/unit%20tests-\\d+%20passing/unit%20tests-${TOTAL}%20passing/g" "$f"
  # Flat-square variants.
  perl -pi -e "s/badge\\/tests-\\d+%20passing/badge\\/tests-${TOTAL}%20passing/g" "$f"
  perl -pi -e "s/unit%20tests-\\d+%20\\(\\d+%20Rust%20%2B%20\\d+%20TS\\)/unit%20tests-${TOTAL}%20(${RUST_TESTS}%20Rust%20%2B%20${FE_TESTS}%20TS)/g" "$f"
done
# Prose + title attributes (language-specific).
perl -pi -e "s/\\*\\*\\d+ unit tests \\(\\d+ Rust \\+ \\d+ frontend\\)\\.\\*\\*/**${TOTAL} unit tests (${RUST_TESTS} Rust + ${FE_TESTS} frontend).**/" README.md
perl -pi -e "s/title=\"Unit tests — \\d+ Rust \\+ \\d+ frontend, all passing\"/title=\"Unit tests — ${RUST_TESTS} Rust + ${FE_TESTS} frontend, all passing\"/" README.md
perl -pi -e "s/\\*\\*\\d+ Unit-Tests \\(\\d+ Rust \\+ \\d+ Frontend\\)\\.\\*\\*/**${TOTAL} Unit-Tests (${RUST_TESTS} Rust + ${FE_TESTS} Frontend).**/" README.de.md
perl -pi -e "s/title=\"Unit-Tests — \\d+ Rust \\+ \\d+ Frontend, alle grün\"/title=\"Unit-Tests — ${RUST_TESTS} Rust + ${FE_TESTS} Frontend, alle grün\"/" README.de.md

echo "✓ Badges updated: ~${LOC_K}k LOC · ${TOTAL} tests (${RUST_TESTS} Rust + ${FE_TESTS} frontend)."
