#!/usr/bin/env bash
# Install (or uninstall) the "boom Audio" virtual audio driver.
#
#   boom-driver-install.sh           # install
#   boom-driver-install.sh --remove  # uninstall
#
# Copies the .driver bundle into /Library/Audio/Plug-Ins/HAL/ and restarts
# coreaudiod (a ~1 s audio blip) — both need admin, so it prompts once via a
# single privileged shell. After install, "boom Audio" appears in
# System Settings → Sound.
set -euo pipefail

HAL="/Library/Audio/Plug-Ins/HAL"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="$ROOT/boom-driver/build/boom-driver.driver"
INSTALLED="$HAL/boom-driver.driver"

if [[ "${1:-}" == "--remove" ]]; then
  osascript -e "do shell script \"rm -rf '$INSTALLED' && killall coreaudiod\" with administrator privileges"
  echo "✓ removed boom Audio driver"
  exit 0
fi

if [[ ! -d "$BUNDLE" ]]; then
  echo "Driver not built. Run: boom-driver/build.sh" >&2
  exit 1
fi

osascript -e "do shell script \"mkdir -p '$HAL' && rm -rf '$INSTALLED' && cp -R '$BUNDLE' '$HAL/' && killall coreaudiod\" with administrator privileges"
echo "✓ installed. 'boom Audio' should now be in System Settings → Sound → Output."
