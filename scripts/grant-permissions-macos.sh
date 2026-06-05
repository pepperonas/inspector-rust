#!/usr/bin/env bash
#
# grant-permissions-macos.sh — streamline Inspector Rust's one-time macOS
# permission setup.
#
# WHAT THIS CAN AND CANNOT DO
#   macOS TCC permissions — Accessibility, Screen Recording, Automation
#   (Apple Events), Microphone — are, BY DESIGN, grantable ONLY by the
#   user. No app or script can flip those toggles for you (the only way
#   around it is disabling SIP and hand-editing the protected TCC
#   database — fragile, version-specific, and a security-bypass; this
#   script deliberately does NOT do that).
#
#   So this script does the next-best thing, in one guided pass:
#     1. resets any stale TCC entries for Inspector Rust (so a re-grant
#        after a code-signature change starts clean),
#     2. (re)launches the app so its own prompts can fire,
#     3. triggers the Automation→Finder prompt,
#     4. opens each relevant System Settings → Privacy pane,
#     5. prints a short checklist of exactly what to toggle.
#
#   You only do this ONCE: install-macos.sh signs with a stable
#   self-signed cert, so the grants survive every future rebuild.
#
# USAGE
#   bash scripts/grant-permissions-macos.sh            # reset + guide
#   bash scripts/grant-permissions-macos.sh --no-reset # just open panes
#
set -euo pipefail

BUNDLE_ID="io.celox.inspector-rust"
APP="/Applications/InspectorRust.app"
DO_RESET=1

for arg in "$@"; do
  case "$arg" in
    --no-reset) DO_RESET=0 ;;
    -h|--help) sed -n '2,/^set -euo pipefail/p' "$0" | sed 's/^# \{0,1\}//' | head -n -1; exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

if [[ ! -d "$APP" ]]; then
  echo "✘ $APP not found — run 'bash scripts/install-macos.sh' first." >&2
  exit 1
fi

step() { printf '\n\033[1m▸ %s\033[0m\n' "$1"; }

# ── 1) Reset stale TCC entries ───────────────────────────────────────────────
# tccutil can RESET (revoke) an app's grant but never GRANT it — resetting
# makes macOS re-prompt cleanly on the next use, which is what you want after
# the ad-hoc→stable-cert switch or any signature change.
if [[ "$DO_RESET" -eq 1 ]]; then
  step "Resetting stale permission entries for ${BUNDLE_ID}"
  for svc in Accessibility ScreenCapture AppleEvents Microphone PostEvent ListenEvent; do
    if tccutil reset "$svc" "$BUNDLE_ID" >/dev/null 2>&1; then
      echo "  • reset $svc"
    fi
  done
fi

# ── 2) Relaunch the app so its own prompts can fire ──────────────────────────
step "Launching Inspector Rust"
pkill -f "${APP}/Contents/MacOS/inspector-rust" 2>/dev/null || true
sleep 1
open "$APP"
sleep 2

# ── 3) Trigger the Automation→Finder prompt ──────────────────────────────────
# Finder selection (Ctrl+Shift+F) asks Finder for its selection via Apple
# Events; the FIRST such call is what makes macOS show the Automation prompt.
# That prompt is keyed to Inspector Rust's own code signature, so it must be
# triggered from inside the app — we can't fire it from this script. Hence
# the instruction below to press the hotkey once.
step "Automation → Finder"
echo "  In Finder, select any file, then press  ⌃⇧F  once in Inspector Rust."
echo "  macOS will prompt to allow controlling Finder — click OK / Allow."

# ── 4) Open each Privacy pane (deep links; Ventura+) ─────────────────────────
step "Opening System Settings → Privacy panes"
open_pane() { open "x-apple.systempreferences:com.apple.preference.security?$1" 2>/dev/null || true; sleep 1.2; }
open_pane "Privacy_Accessibility"
open_pane "Privacy_ScreenCapture"
open_pane "Privacy_Automation"
open_pane "Privacy_Microphone"

# ── 5) Checklist ─────────────────────────────────────────────────────────────
cat <<'EOF'

────────────────────────────────────────────────────────────────────────
  Grant these for Inspector Rust (toggle ON in each pane that just opened):

    • Accessibility    — paste, text expander, input lock   (REQUIRED)
    • Screen Recording — OCR (⌃⇧O) + screenshot (⌃⇧S)
    • Automation › Finder — Finder selection (⌃⇧F) + Markdown→PDF (⌃⇧M)
    • Microphone       — BPM detector (type `bpm`)           (optional)

  Why no fully-automatic toggle? macOS only lets the USER grant these —
  that's the whole point of TCC. After you grant Accessibility once, the
  app's Settings tab shows a green "Restart" prompt; click it and you're
  set. The stable self-signed cert keeps every grant across rebuilds.
────────────────────────────────────────────────────────────────────────
EOF
