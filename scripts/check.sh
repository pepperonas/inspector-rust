#!/usr/bin/env bash
# scripts/check.sh — run the full static-analysis bundle across the workspace.
# Usage: bash scripts/check.sh
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

if cargo clippy --version >/dev/null 2>&1; then
  # The vendored OpenScreen compositor is upstream code (~158 clippy findings in
  # its tests) and is not held to this repo's lint bar; its own tests stay
  # runnable on demand — see CLAUDE.md "record — stage 2".
  echo "▶ cargo clippy (workspace, without the vendored openscreen-compositor)"
  cargo clippy --workspace --exclude openscreen-compositor --all-targets -- -D warnings
else
  echo "▶ cargo check (workspace) — clippy not installed"
  echo "  (install via: rustup component add clippy)"
  cargo check --workspace --exclude openscreen-compositor --all-targets
fi

echo "▶ pnpm typecheck"
pnpm typecheck

echo "▶ pnpm lint"
pnpm lint

echo "▶ gen-docs --check (README command matrix in sync with the CommandDoc registry)"
node scripts/gen-docs.mjs --check

echo "▶ vendor-badges --check (static README badges live in .github/badges, not on shields.io)"
node scripts/vendor-badges.mjs --check

echo "▶ frontend bundle budget (PERFORMANCE-PLAN D3b: eager App chunk must stay lazy-split)"
pnpm --filter inspector-rust-frontend build >/dev/null
node scripts/check-bundle.mjs

echo "✓ all checks passed"
