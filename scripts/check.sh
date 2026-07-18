#!/usr/bin/env bash
# scripts/check.sh — run the full static-analysis bundle across the workspace.
# Usage: bash scripts/check.sh
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
cd "$here"

if cargo clippy --version >/dev/null 2>&1; then
  echo "▶ cargo clippy (workspace)"
  cargo clippy --workspace --all-targets -- -D warnings
else
  echo "▶ cargo check (workspace) — clippy not installed"
  echo "  (install via: rustup component add clippy)"
  cargo check --workspace --all-targets
fi

echo "▶ pnpm typecheck"
pnpm typecheck

echo "▶ pnpm lint"
pnpm lint

echo "▶ gen-docs --check (README command matrix in sync with the CommandDoc registry)"
node scripts/gen-docs.mjs --check

echo "✓ all checks passed"
