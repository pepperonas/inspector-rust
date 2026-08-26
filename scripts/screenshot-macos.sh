#!/usr/bin/env bash
# Capture one popup screenshot for docs/screenshots/ (macOS).
#
#   scripts/screenshot-macos.sh disk "disk ~/claude/inspector-rust" 6
#   scripts/screenshot-macos.sh weather "weather berlin" 4 --no-enter
#
# Args: <name> <query> [seconds-to-settle] [--no-enter]
#
# ⚠️ Focus is the whole trick. The keystrokes are synthetic, so they land in
# whatever app is frontmost — and a Screen Sharing window forwards them to the
# REMOTE machine, where they end up typed into some innocent text field (this
# happened; two stray messages were sent). The script therefore brings Finder
# to the front first and REFUSES to type if that didn't take.
#
# Needs: Automation + Accessibility for whatever runs this (Terminal/iTerm).
set -euo pipefail

name=${1:?usage: screenshot-macos.sh <name> <query> [wait] [--no-enter]}
query=${2:?missing query}
wait_s=${3:-5}
press_enter=1
preview_only=0
for opt in "${@:4}"; do
  case "$opt" in
    --no-enter) press_enter=0 ;;
    --preview-only) preview_only=1 ;;
  esac
done

out_dir="$(cd "$(dirname "$0")/.." && pwd)/docs/screenshots"
out="$out_dir/$name.png"
mkdir -p "$out_dir"

say() { printf '▸ %s\n' "$*"; }

pgrep -x inspector-rust >/dev/null || { echo "✗ Inspector Rust läuft nicht"; exit 1; }

# 1. Get focus onto a LOCAL app, and verify it — never type blind.
# The terminal running this script often wins the first activate back, so try
# a few times before giving up — but NEVER type without confirming.
for _ in 1 2 3; do
  osascript -e 'tell application "Finder" to activate' >/dev/null
  sleep 0.8
  front=$(osascript -e 'tell application "System Events" to name of first process whose frontmost is true')
  [[ "$front" == "Finder" ]] && break
done
if [[ "$front" != "Finder" ]]; then
  echo "✗ Frontmost ist '$front', nicht Finder — Tastenanschläge gingen woanders hin."
  echo "  Bring den lokalen Schreibtisch nach vorn (Screen Sharing schluckt sie) und starte neu."
  exit 1
fi

# 2. Open the popup (Ctrl+Space) and confirm the window is really there.
#
# ⚠️ Close it FIRST if it is still up: the hotkey TOGGLES, so a leftover popup
# from the previous capture means the next run closes it instead of opening it
# (that is exactly how the `alias` shot failed). Esc alone is not enough —
# inside a panel the first Esc only steps back a level.
popup_open() {
  osascript -e 'tell application "System Events" to tell process "inspector-rust" to count windows' 2>/dev/null | grep -qv '^0$'
}
for _ in 1 2 3; do
  popup_open || break
  osascript -e 'tell application "System Events" to key code 53' >/dev/null
  sleep 0.4
done

say "Popup öffnen"
osascript -e 'tell application "System Events" to key code 49 using control down' >/dev/null
sleep 1.4
read_bounds() {
  osascript -e 'tell application "System Events" to tell process "inspector-rust" to get {value of attribute "AXPosition", value of attribute "AXSize"} of window 1' 2>/dev/null || true
}
bounds=$(read_bounds)
if [[ -z "$bounds" ]]; then
  # The toggle occasionally lands on the wrong side of an in-flight open/close
  # animation. One more press settles it; two failures mean something real.
  say "kein Fenster — Hotkey wiederholen"
  osascript -e 'tell application "System Events" to key code 49 using control down' >/dev/null
  sleep 1.4
  bounds=$(read_bounds)
fi
[[ -n "$bounds" ]] || { echo "✗ Kein Popup-Fenster — Hotkey kam nicht an."; exit 1; }

# 3. Type the query (select-all first: a previous query may still stand).
#
# ⚠️ ONE character at a time, with a generous pause. A single
# `keystroke "disk ~/x"` outruns the React input and the letters arrive
# SHUFFLED — the first attempt produced `di~claude/insk /spector-rust` and
# photographed "No matches". 40 ms then still DROPPED a slash on a slower
# display (`~claude//inspector-rust`), so it is 90 ms. Slow beats re-shooting.
say "Eingabe: $query"
osascript -e 'tell application "System Events" to keystroke "a" using command down' >/dev/null
esc_query=${query//\\/\\\\}
esc_query=${esc_query//\"/\\\"}
osascript <<APPLESCRIPT >/dev/null
tell application "System Events"
  repeat with c in characters of "$esc_query"
    keystroke (c as text)
    delay 0.09
  end repeat
end tell
APPLESCRIPT
sleep 0.8
if [[ $press_enter -eq 1 ]]; then
  osascript -e 'tell application "System Events" to key code 36' >/dev/null
fi

say "Warte ${wait_s}s (Scan/Animation)"
sleep "$wait_s"

# 4. Capture exactly the popup rectangle (points; retina gives 2× pixels).
bounds=$(osascript -e 'tell application "System Events" to tell process "inspector-rust" to get {value of attribute "AXPosition", value of attribute "AXSize"} of window 1')
IFS=', ' read -r x y w h <<<"$bounds"
if [[ $preview_only -eq 1 ]]; then
  # ⚠️ Privacy: the LEFT column is the live clipboard history — real clips,
  # real text. These images go into a public repo, so any shot whose query
  # leaves clips visible must be taken preview-only. (`disk`, `clown`, `repo`
  # filter the list down to nothing and are safe as full-window shots.)
  # The divider sits at 338 of the 840pt "large" preset; scale it.
  split=$(( w * 338 / 840 ))
  x=$(( x + split )); w=$(( w - split ))
  say "Nur Vorschau ${w}×${h} @ ${x},${y}"
else
  say "Fenster ${w}×${h} @ ${x},${y}"
fi
screencapture -x -R "$x,$y,$w,$h" "$out"

# 5. Leave the popup closed again.
osascript -e 'tell application "System Events" to key code 53' >/dev/null || true

px=$(sips -g pixelWidth -g pixelHeight "$out" | tail -2 | tr -d ' \n')
echo "✓ $out ($px)"
