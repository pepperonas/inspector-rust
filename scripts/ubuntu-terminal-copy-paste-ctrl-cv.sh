#!/usr/bin/env bash
# Optional manual fallback — Inspector Rust 0.25.1+ does this automatically on
# first start (see desktop_shortcuts.rs: reconcile_terminal_copy_paste).
# Move GNOME Terminal copy/paste from Ctrl+Shift+C/V to Ctrl+C/V so they do not
# clash with Inspector Rust (popup Ctrl+Shift+V, color Ctrl+Shift+C).
#
# Note: In the terminal, Ctrl+C still sends SIGINT when nothing is selected to copy
# (normal GNOME Terminal behaviour with Control+c as copy binding).
set -euo pipefail

if ! command -v gsettings >/dev/null 2>&1; then
  echo "gsettings not found — this script is for GNOME/Ubuntu."
  exit 1
fi

DEFAULT="$(gsettings get org.gnome.Terminal.ProfilesList default | tr -d "'")"
if [ -z "$DEFAULT" ]; then
  echo "No default GNOME Terminal profile found."
  exit 1
fi

SCHEMA="org.gnome.Terminal.Legacy.Keybindings:/org/gnome/terminal/legacy/profiles:/${DEFAULT}/"

COPY_BEFORE="$(gsettings get "$SCHEMA" copy 2>/dev/null || echo "?")"
PASTE_BEFORE="$(gsettings get "$SCHEMA" paste 2>/dev/null || echo "?")"
TARGET_COPY="'<Control>c'"
TARGET_PASTE="'<Control>v'"

echo "==> GNOME Terminal profile: $DEFAULT"
echo "    Copy:  $COPY_BEFORE"
echo "    Paste: $PASTE_BEFORE"

if [ "$COPY_BEFORE" = "$TARGET_COPY" ] && [ "$PASTE_BEFORE" = "$TARGET_PASTE" ]; then
  echo ""
  echo "Already configured — nothing to change."
else
  gsettings set "$SCHEMA" copy '<Control>c'
  gsettings set "$SCHEMA" paste '<Control>v'
  echo ""
  echo "Updated to Ctrl+C / Ctrl+V."
fi
echo ""
echo "Done. Inspector Rust keeps:"
echo "  Ctrl+Shift+V  — clipboard popup"
echo "  Ctrl+Shift+C  — pick color"
echo "  Ctrl+Shift+O  — OCR"
echo "  Ctrl+Shift+S  — screenshot"
echo ""
echo "Other apps already use Ctrl+C / Ctrl+V for copy/paste by default."
echo "If you had extra Custom Shortcuts for Kopieren/Einfügen on Ctrl+Shift+C/V,"
echo "remove them: Settings → Keyboard → Custom Shortcuts."
