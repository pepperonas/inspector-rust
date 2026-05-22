#!/usr/bin/env bash
# Register Inspector Rust shortcuts in GNOME/Cinnamon (Wayland).
# The app also runs this logic on first launch; this script is for install-linux.sh.
set -euo pipefail

if ! command -v gsettings >/dev/null 2>&1; then
  echo "gsettings not found — skip desktop shortcut setup"
  exit 0
fi

SESSION="${XDG_SESSION_TYPE:-}"
DESKTOP="${XDG_CURRENT_DESKTOP:-}"

if [ "$SESSION" = "x11" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  echo "==> X11: in-app global shortcuts (Ctrl+Shift+…) — no gsettings needed"
  exit 0
fi

case "$DESKTOP" in
  *cinnamon*|*Cinnamon*|*X-Cinnamon*)
    LIST_SCHEMA="org.cinnamon.desktop.keybindings"
    LIST_KEY="custom-list"
    CUSTOM_SCHEMA="org.cinnamon.keybindings.custom-keybinding"
    PATH_PREFIX="/org/cinnamon/desktop/keybindings/custom-keybindings/inspector-rust-"
    ;;
  *gnome*|*GNOME*|*ubuntu*|*Unity*|"")
    LIST_SCHEMA="org.gnome.settings-daemon.plugins.media-keys"
    LIST_KEY="custom-keybindings"
    CUSTOM_SCHEMA="org.gnome.settings-daemon.plugins.media-keys.custom-keybinding"
    PATH_PREFIX="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/inspector-rust-"
    ;;
  *)
    echo "==> Desktop '$DESKTOP': automatic shortcut setup not supported (use tray or manual bindings)"
    exit 0
    ;;
esac

if command -v inspector-rust >/dev/null 2>&1; then
  CMD="$(command -v inspector-rust)"
else
  echo "==> inspector-rust not in PATH — shortcuts will be registered on first app start after install"
  exit 0
fi

echo "==> Registering custom shortcuts ($SESSION / $DESKTOP) → $CMD"

declare -a PATHS=()
install_one() {
  local id="$1" name="$2" arg="$3" binding="$4"
  local path="${PATH_PREFIX}${id}/"
  PATHS+=("$path")
  gsettings set "${CUSTOM_SCHEMA}:${path}" name "$name"
  gsettings set "${CUSTOM_SCHEMA}:${path}" command "$CMD $arg"
  gsettings set "${CUSTOM_SCHEMA}:${path}" binding "$binding"
}

install_one toggle "Inspector Rust — Open" "--toggle-popup" "<Control><Shift>v"
install_one ocr "Inspector Rust — OCR" "--ocr" "<Control><Shift>o"
install_one screenshot "Inspector Rust — Screenshot" "--screenshot" "<Control><Shift>s"
install_one color "Inspector Rust — Pick color" "--pick-color" "<Control><Shift>c"

# Merge with existing custom bindings (drop old inspector-rust-* entries first)
existing="$(gsettings get "$LIST_SCHEMA" "$LIST_KEY")"
merged=()
if [ "$existing" != "@as []" ]; then
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    p="${line//\'/}"
    [[ "$p" == *inspector-rust-* ]] && continue
    merged+=("$p")
  done < <(echo "$existing" | tr -d '[]' | tr ',' '\n' | sed "s/^ *'//;s/' *$//")
fi
for p in "${PATHS[@]}"; do merged+=("$p"); done

arr="@as ["
first=1
for p in "${merged[@]}"; do
  [ $first -eq 1 ] || arr+=", "
  arr+="'$p'"
  first=0
done
arr+="]"
gsettings set "$LIST_SCHEMA" "$LIST_KEY" "$arr"

echo "    Done. Check: Settings → Keyboard → Custom Shortcuts (Inspector Rust — …)"
gsettings get "$LIST_SCHEMA" "$LIST_KEY"
