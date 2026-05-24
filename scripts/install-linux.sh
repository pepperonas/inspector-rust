#!/usr/bin/env bash
# Install build/runtime dependencies for Inspector Rust on Ubuntu/Debian.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> System packages (Tauri / GTK / clipboard / screenshots)"
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  curl \
  git \
  pkg-config \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  libxdo-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  scrot \
  tesseract-ocr \
  tesseract-ocr-deu \
  tesseract-ocr-eng \
  fuse3 libfuse2t64

# Wayland users: optional region capture (grim + slurp)
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
  echo "==> Wayland detected — installing grim + slurp (optional region picker)"
  sudo apt-get install -y grim slurp || true
fi

echo "==> Node.js 20 (via NodeSource, if missing)"
node_major() {
  node -p 'parseInt(process.versions.node.split(".")[0], 10)' 2>/dev/null || echo 0
}
if ! command -v node >/dev/null 2>&1 || [ "$(node_major)" -lt 20 ]; then
  curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
  sudo apt-get install -y nodejs
fi

echo "==> pnpm"
if ! command -v pnpm >/dev/null 2>&1; then
  sudo npm install -g pnpm@10
fi

echo "==> Rust (rustup stable, if cargo is missing or too old)"
need_rustup=false
if ! command -v cargo >/dev/null 2>&1; then
  need_rustup=true
elif ! cargo --version 2>/dev/null | grep -qE 'cargo 1\.(7[7-9]|[89][0-9]|[1-9][0-9]{2,})'; then
  echo "    System cargo is older than 1.77 — installing rustup stable"
  need_rustup=true
fi

if $need_rustup; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

export PATH="${HOME}/.cargo/bin:${PATH}"

echo "==> pnpm install"
pnpm install

echo ""
echo "Done. Next steps:"
echo "  source \"\$HOME/.cargo/env\"   # if rustup was just installed"
echo "  pnpm dev:linux                # run in development"
echo "  pnpm build:linux              # .deb under target/release/bundle/deb/"
echo ""
echo "Data directory: ~/.local/share/InspectorRust/history.db"
echo "See linux/README.md for Wayland shortcuts and optional tools."

if [ -n "${WAYLAND_DISPLAY:-}" ] || [ "${XDG_SESSION_TYPE:-}" = "wayland" ]; then
  echo ""
  echo "==> Desktop shortcuts (GNOME/Cinnamon Wayland)"
  if command -v inspector-rust >/dev/null 2>&1; then
    inspector-rust --setup-shortcuts || true
    echo "    Ran inspector-rust --setup-shortcuts (conflict scan + install)."
    echo "    Review or change keys in Settings → Linux desktop shortcuts."
  elif [ -x "$ROOT/target/release/inspector-rust" ]; then
    "$ROOT/target/release/inspector-rust" --setup-shortcuts || true
    echo "    Ran target/release/inspector-rust --setup-shortcuts."
  else
    bash "$ROOT/scripts/install-desktop-shortcuts.sh" || true
    echo "    Dev fallback script only — after build/install, run:"
    echo "      inspector-rust --setup-shortcuts"
    echo "    Or open Settings → Linux desktop shortcuts (scan, record, save)."
  fi
  echo "    First app launch also auto-configures shortcuts when gsettings is available."
fi
