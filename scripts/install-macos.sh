#!/usr/bin/env bash
#
# install-macos.sh — build, sign, and install Inspector Rust into /Applications.
#
# WHY THIS SCRIPT EXISTS:
#   Inspector Rust has no Apple Developer ID. macOS TCC (Accessibility,
#   Screen Recording) keys a permission grant to the app's *code
#   signature*. A plain `tauri build` leaves the binary ad-hoc-signed,
#   and macOS keys an ad-hoc grant to the cdhash — the binary hash —
#   which changes on every rebuild. So "rebuild → install → grant →
#   rebuild" used to silently invalidate the grant on every iteration.
#
#   THE FIX — a stable self-signed certificate:
#   This script creates (once) a self-signed code-signing certificate in
#   a dedicated, script-managed keychain
#   (~/Library/Keychains/inspector-rust-signing.keychain-db) and signs
#   every build with it. With a real certificate — even self-signed —
#   TCC keys the grant to the app's *Designated Requirement*:
#
#       identifier "io.celox.inspector-rust" and
#       certificate leaf = H"<stable cert hash>"
#
#   That requirement does NOT reference the cdhash, so it stays constant
#   across rebuilds. Net effect: you grant Accessibility + Screen
#   Recording ONCE and the grant survives every future build.
#
#   The certificate is created fully non-interactively — no admin
#   password, no GUI "Always Allow" prompt. Its keychain has a hard-coded
#   local password: that keychain holds nothing but a self-signed
#   code-signing key, worthless off this machine and granting access to
#   nothing, so the password is not a secret.
#
#   ONE-TIME re-grant: the first install after switching from ad-hoc to
#   the self-signed cert needs a single re-grant — the stale ad-hoc TCC
#   entry won't match the new signature. After that it sticks. The in-app
#   Settings panel auto-detects the grant and offers a one-click relaunch.
#
#   If certificate creation fails for any reason, the script falls back
#   to ad-hoc signing (the previous behaviour) so it never hard-fails.
#
#   The build itself is still skipped when the source tree is unchanged
#   (SHA-256 over .rs / .ts / .tsx / .json / locks / entitlements) — now
#   purely a build-time optimisation, since the signature is stable
#   regardless. We hash the *source*, not the binary, because Rust
#   release builds aren't byte-reproducible.
#
# USAGE:
#   bash scripts/install-macos.sh         # build + install (idempotent) + launch
#   bash scripts/install-macos.sh --reset # …also tccutil-reset stale grants
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_ID="io.celox.inspector-rust"
APP_NAME="InspectorRust.app"
BUILD_OUT="${REPO_ROOT}/target/release/bundle/macos/${APP_NAME}"
INSTALL_PATH="/Applications/${APP_NAME}"
ENTITLEMENTS="${REPO_ROOT}/macos/src-tauri/entitlements.plist"

# Stable self-signed signing — see the header block for the full why.
SIGN_KEYCHAIN="${HOME}/Library/Keychains/inspector-rust-signing.keychain-db"
SIGN_CERT_CN="Inspector Rust Local Code Signing"
# Hard-coded on purpose: this keychain holds only a self-signed
# code-signing key that is worthless off this machine. A fixed password
# is what makes signing fully non-interactive (no admin password, no
# GUI prompt). It is not a secret.
SIGN_KEYCHAIN_PW="inspector-rust-local"

DO_RESET=0
for arg in "$@"; do
  case "$arg" in
    --reset) DO_RESET=1 ;;
    -h|--help)
      sed -n '2,/^set -euo pipefail/p' "$0" | sed 's/^# \{0,1\}//' | head -n -1
      exit 0
      ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

# ── helpers ─────────────────────────────────────────────────────────────────

bin_sha256() {
  # SHA-256 of the actual Mach-O — not the .app bundle wrapper.
  # NB: bash 3.2 (the macOS default) propagates non-zero exits from
  # command substitutions in `set -e` mode, so every helper here ends
  # with `return 0` to keep the caller's `var=$(helper)` from blowing up
  # the whole script when the helper just had nothing to print.
  local app="$1"
  local exe="${app}/Contents/MacOS/inspector-rust"
  if [[ -f "${exe}" ]]; then
    shasum -a 256 "${exe}" | cut -d' ' -f1
  fi
  return 0
}

# SHA-256 over every input that meaningfully affects the built binary.
# Rust release builds aren't byte-reproducible (paths, timestamps, LLVM
# non-determinism), so comparing the *output* across rebuilds always
# differs. Comparing the *inputs* gives us a reliable "did anything that
# matters actually change?" signal — and lets us skip the build itself,
# not just re-signing, when nothing did.
source_sha256() {
  (
    cd "${REPO_ROOT}"
    {
      find core/rust-lib/src core/rust-lib/Cargo.toml \
           core/frontend/src core/frontend/index.html \
           core/frontend/package.json core/frontend/tsconfig.json \
           core/frontend/vite.config.ts \
           macos/src-tauri/src macos/src-tauri/Cargo.toml \
           macos/src-tauri/tauri.conf.json macos/src-tauri/entitlements.plist \
           macos/src-tauri/build.rs macos/src-tauri/capabilities \
           Cargo.toml Cargo.lock pnpm-lock.yaml \
           -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' \
                   -o -name '*.json' -o -name '*.html' -o -name '*.toml' \
                   -o -name '*.lock' -o -name '*.yaml' -o -name '*.css' \
                   -o -name 'entitlements.plist' \) \
           2>/dev/null
    } | LC_ALL=C sort | xargs shasum -a 256 2>/dev/null | shasum -a 256 | cut -d' ' -f1
  )
}

stored_source_hash() {
  local app="$1"
  local f="${app}/Contents/Resources/.inspector-rust-source-hash"
  [[ -f "${f}" ]] && cat "${f}"
  return 0
}

write_source_hash() {
  local app="$1"
  local hash="$2"
  echo -n "${hash}" > "${app}/Contents/Resources/.inspector-rust-source-hash"
}

cdhash() {
  local app="$1"
  if [[ -d "${app}" ]]; then
    # `--verbose=4` is required to get the `CDHash=...` line; -dv alone
    # only prints `CodeDirectory v=… size=… flags=… hashes=… location=…`.
    codesign -dvvv "${app}" 2>&1 | sed -n 's/^CDHash=//p'
  fi
  return 0
}

current_identifier() {
  local app="$1"
  if [[ -d "${app}" ]]; then
    codesign -dv "${app}" 2>&1 | sed -n 's/^Identifier=//p'
  fi
  return 0
}

kill_running() {
  local pids
  pids=$(pgrep -f "${INSTALL_PATH}/Contents/MacOS/inspector-rust" || true)
  if [[ -n "${pids}" ]]; then
    echo "  stopping Inspector Rust PIDs: ${pids}"
    kill ${pids} 2>/dev/null || true
    sleep 1
  fi
}

# Add our signing keychain to the user's keychain search list (idempotent).
# codesign only resolves a `--sign <name>` identity from keychains on the
# search list — its `--keychain` flag is unreliable for this — so the
# dedicated keychain has to be on the list for signing to find the cert.
# Non-destructive: every existing keychain is preserved, ours is appended.
add_keychain_to_search_list() {
  local kc="$1"
  if security list-keychains -d user | grep -qF "${kc}"; then
    return 0
  fi
  local -a list=()
  local line
  while IFS= read -r line; do
    line="${line#"${line%%[![:space:]]*}"}"   # strip leading whitespace
    line="${line%\"}"; line="${line#\"}"        # strip surrounding quotes
    [[ -n "${line}" ]] && list+=("${line}")
  done < <(security list-keychains -d user)
  security list-keychains -d user -s "${list[@]}" "${kc}" 2>/dev/null || true
}

# Ensure the stable self-signed signing certificate exists, creating it
# once on first run, and that it's usable by codesign. Echoes the
# signing identity to use on stdout:
#   • the certificate common-name  → sign with the stable cert
#   • "-"                          → ad-hoc fallback (cert unavailable)
# All progress text goes to stderr so the caller can capture the
# identity cleanly. Always returns 0 — failure just means ad-hoc.
ensure_signing_cert() {
  # ── Create the cert + keychain once, if not already present ───────────
  if [[ ! -f "${SIGN_KEYCHAIN}" ]] \
     || ! security find-certificate -c "${SIGN_CERT_CN}" "${SIGN_KEYCHAIN}" >/dev/null 2>&1; then
    echo "▸ Creating a stable self-signed signing certificate (one-time)…" >&2

    local tmp
    tmp="$(mktemp -d)" || { echo "-"; return 0; }

    cat > "${tmp}/cert.cnf" <<'CNF'
[ req ]
distinguished_name = dn
x509_extensions    = v3
prompt             = no
[ dn ]
CN = Inspector Rust Local Code Signing
[ v3 ]
basicConstraints   = critical, CA:false
keyUsage           = critical, digitalSignature
extendedKeyUsage   = critical, codeSigning
CNF

    if ! openssl req -x509 -newkey rsa:2048 -nodes \
          -keyout "${tmp}/key.pem" -out "${tmp}/cert.pem" \
          -days 3650 -config "${tmp}/cert.cnf" >/dev/null 2>&1; then
      echo "  ⚠ openssl cert generation failed — falling back to ad-hoc signing" >&2
      rm -rf "${tmp}"; echo "-"; return 0
    fi
    # Legacy PKCS#12 algorithms (SHA1/3DES) — macOS' `security import`
    # cannot read the AES-256/SHA-256 default that OpenSSL 3 produces
    # ("MAC verification failed"). These flags work on OpenSSL 3 and
    # LibreSSL alike.
    if ! openssl pkcs12 -export -name "${SIGN_CERT_CN}" \
          -inkey "${tmp}/key.pem" -in "${tmp}/cert.pem" \
          -out "${tmp}/cert.p12" -passout "pass:${SIGN_KEYCHAIN_PW}" \
          -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg SHA1 >/dev/null 2>&1; then
      echo "  ⚠ openssl p12 export failed — falling back to ad-hoc signing" >&2
      rm -rf "${tmp}"; echo "-"; return 0
    fi

    # Dedicated keychain — created if absent, no auto-lock timeout.
    if [[ ! -f "${SIGN_KEYCHAIN}" ]]; then
      if ! security create-keychain -p "${SIGN_KEYCHAIN_PW}" "${SIGN_KEYCHAIN}" 2>/dev/null; then
        echo "  ⚠ keychain create failed — falling back to ad-hoc signing" >&2
        rm -rf "${tmp}"; echo "-"; return 0
      fi
    fi
    security set-keychain-settings "${SIGN_KEYCHAIN}" 2>/dev/null || true
    security unlock-keychain -p "${SIGN_KEYCHAIN_PW}" "${SIGN_KEYCHAIN}" 2>/dev/null || true

    # `-A` → any tool may use the key (no per-app ACL prompt).
    if ! security import "${tmp}/cert.p12" -k "${SIGN_KEYCHAIN}" \
          -P "${SIGN_KEYCHAIN_PW}" -A >/dev/null 2>&1; then
      echo "  ⚠ keychain import failed — falling back to ad-hoc signing" >&2
      rm -rf "${tmp}"; echo "-"; return 0
    fi
    # Clear the key's partition list so codesign never shows a password
    # dialog — our keychain, our known password → fully non-interactive.
    security set-key-partition-list -S apple-tool:,apple: \
      -s -k "${SIGN_KEYCHAIN_PW}" "${SIGN_KEYCHAIN}" >/dev/null 2>&1 || true

    # Trust the cert for the code-signing policy — without this the
    # identity is "not trusted" and codesign refuses it. User-domain
    # trust needs no admin password (no `-d`, no sudo).
    if ! security add-trusted-cert -r trustRoot -p codeSign \
          "${tmp}/cert.pem" >/dev/null 2>&1; then
      echo "  ⚠ could not trust the certificate — falling back to ad-hoc" >&2
      rm -rf "${tmp}"; echo "-"; return 0
    fi

    rm -rf "${tmp}"
    echo "  ✓ certificate created + trusted" >&2
  fi

  # ── Idempotent every-run setup: unlock + search-list membership ───────
  security unlock-keychain -p "${SIGN_KEYCHAIN_PW}" "${SIGN_KEYCHAIN}" 2>/dev/null || true
  add_keychain_to_search_list "${SIGN_KEYCHAIN}"

  # Final gate — codesign only signs with a *valid* (trusted) identity.
  if security find-identity -v -p codesigning "${SIGN_KEYCHAIN}" 2>/dev/null \
       | grep -qF "${SIGN_CERT_CN}"; then
    echo "${SIGN_CERT_CN}"
  else
    echo "  ⚠ signing identity not valid — falling back to ad-hoc" >&2
    echo "-"
  fi
  return 0
}

resign_app() {
  local app="$1"
  local sign_id="$2"
  local -a args
  args=(--force --deep --sign "${sign_id}" --identifier "${BUNDLE_ID}")
  if [[ -f "${ENTITLEMENTS}" ]]; then
    args+=(--entitlements "${ENTITLEMENTS}" --options runtime)
  fi
  # The signing keychain is unlocked + on the search list by
  # ensure_signing_cert, so codesign resolves the identity by name.
  if [[ "${sign_id}" != "-" ]]; then
    security unlock-keychain -p "${SIGN_KEYCHAIN_PW}" "${SIGN_KEYCHAIN}" 2>/dev/null || true
  fi
  codesign "${args[@]}" "${app}"
}

reset_tcc() {
  echo "▸ Resetting stale TCC grants for ${BUNDLE_ID}…"
  tccutil reset Accessibility "${BUNDLE_ID}" 2>/dev/null || true
  tccutil reset PostEvent     "${BUNDLE_ID}" 2>/dev/null || true
  tccutil reset ScreenCapture "${BUNDLE_ID}" 2>/dev/null || true
  echo "  → next launch re-prompts for Accessibility + Screen Recording."
}

# ── 1) source hash — decide *before* building whether we even need to ──────

echo "▸ Hashing source tree…"
NEW_SRC_HASH="$(source_sha256)"
OLD_SRC_HASH="$(stored_source_hash "${INSTALL_PATH}")"
INSTALLED_ID="$(current_identifier "${INSTALL_PATH}")"

UNCHANGED=0
if [[ -n "${NEW_SRC_HASH}" \
   && -n "${OLD_SRC_HASH}" \
   && "${NEW_SRC_HASH}" == "${OLD_SRC_HASH}" \
   && "${INSTALLED_ID}" == "${BUNDLE_ID}" \
   && -d "${INSTALL_PATH}" ]]; then
  UNCHANGED=1
fi

if [[ "${UNCHANGED}" -eq 1 ]]; then
  echo "▸ Source unchanged (sha256: ${NEW_SRC_HASH:0:12}…) — skipping build"
  echo "  Identifier: ${INSTALLED_ID}"
  echo "  cdhash:     $(cdhash "${INSTALL_PATH}")"
  echo "  → existing TCC grant survives (skipped build keeps the signature)"

  kill_running
  if [[ "${DO_RESET}" -eq 1 ]]; then
    reset_tcc
  fi
  echo "▸ Launching…"
  open "${INSTALL_PATH}"
  echo
  echo "✓ Re-launched $(defaults read "${INSTALL_PATH}/Contents/Info.plist" CFBundleShortVersionString) at ${INSTALL_PATH}"
  exit 0
fi

# ── 2) source changed — build ───────────────────────────────────────────────

echo "▸ Source changed (or first install) — full build + install"
[[ -n "${OLD_SRC_HASH}" ]] && echo "  old src sha256: ${OLD_SRC_HASH:0:12}…"
echo "  new src sha256: ${NEW_SRC_HASH:0:12}…"
echo "  ⚠ cdhash WILL change — TCC Accessibility grant must be re-given"
echo "    (the app's Settings panel auto-detects this and walks you"
echo "     through the steps; takes ~30 seconds)"

echo "▸ Building InspectorRust.app (release, --bundles app)…"
cd "${REPO_ROOT}"
pnpm --filter inspector-rust-macos tauri build --bundles app

if [[ ! -d "${BUILD_OUT}" ]]; then
  echo "✘ build output missing: ${BUILD_OUT}" >&2
  exit 1
fi

# ── 3) install ──────────────────────────────────────────────────────────────

kill_running

echo "▸ Replacing ${INSTALL_PATH}…"
rm -rf "${INSTALL_PATH}"
cp -R "${BUILD_OUT}" "${INSTALL_PATH}"

# Quarantine attribute: set by `cp` from a different volume; it triggers
# Gatekeeper "are you sure you want to open" and *also* invalidates the
# code signature for some macOS versions.
xattr -dr com.apple.quarantine "${INSTALL_PATH}" 2>/dev/null || true

# Inject NSAppleEventsUsageDescription into the bundled Info.plist.
# Tauri 2's bundler has no first-class field for arbitrary Info.plist
# keys, so we add this one with plutil before (re-)signing. The text
# is what macOS shows the user in the Automation TCC prompt the first
# time Ctrl+Shift+F asks Finder for its selection (v0.30.0+).
INFO_PLIST="${INSTALL_PATH}/Contents/Info.plist"
if [[ -f "${INFO_PLIST}" ]]; then
  AE_DESC='Inspector Rust uses Apple Events to read your Finder selection so the popup can run actions (resize, OCR, …) on the files you have selected.'
  # `-replace` works whether or not the key already exists.
  if plutil -replace NSAppleEventsUsageDescription -string "${AE_DESC}" \
       "${INFO_PLIST}" 2>/dev/null; then
    echo "▸ Injected NSAppleEventsUsageDescription into Info.plist"
  else
    echo "  ⚠ plutil replace of NSAppleEventsUsageDescription failed — Automation prompt may show generic copy"
  fi
fi

# Stamp the source hash *before* signing so the file is covered by the
# code signature's resource seal. Writing it afterwards adds an unsealed
# file to Contents/Resources/ — `codesign --verify` then fails with "a
# sealed resource is missing or invalid", which can invalidate the
# signature macOS evaluates for the TCC grant.
write_source_hash "${INSTALL_PATH}" "${NEW_SRC_HASH}"

echo "▸ Signing with stable identifier ${BUNDLE_ID}…"
SIGN_ID="$(ensure_signing_cert)"
if [[ "${SIGN_ID}" == "-" ]]; then
  echo "  ⚠ signing ad-hoc (fallback) — TCC grant will need re-giving on rebuild"
else
  echo "  signing with stable self-signed cert: ${SIGN_ID}"
fi
if [[ -f "${ENTITLEMENTS}" ]]; then
  echo "  using entitlements: ${ENTITLEMENTS}"
fi
resign_app "${INSTALL_PATH}" "${SIGN_ID}"

if [[ "${DO_RESET}" -eq 1 ]]; then
  reset_tcc
fi

echo "▸ Verifying signature…"
if codesign --verify --strict "${INSTALL_PATH}" 2>&1 | sed 's/^/  /'; then
  echo "  ✓ signature verifies (seal intact)"
else
  echo "  ⚠ codesign --verify failed — the TCC grant may not stick"
fi
codesign -dv "${INSTALL_PATH}" 2>&1 | sed 's/^/  /'
echo "  Identifier:        $(current_identifier "${INSTALL_PATH}")"
echo "  cdhash this build: $(cdhash "${INSTALL_PATH}")"
echo "  source hash:       ${NEW_SRC_HASH:0:12}…"
if [[ "${SIGN_ID}" != "-" ]]; then
  DR="$(codesign -d -r- "${INSTALL_PATH}" 2>&1 | sed -n 's/^designated => //p')"
  echo "  designated req:    ${DR}"
  echo "  → this requirement is cdhash-free — the TCC grant survives rebuilds"
fi

echo "▸ Launching…"
open "${INSTALL_PATH}"

echo
echo "✓ Installed $(defaults read "${INSTALL_PATH}/Contents/Info.plist" CFBundleShortVersionString) at ${INSTALL_PATH}"
echo
if [[ "${SIGN_ID}" != "-" ]]; then
  echo "Signed with the stable self-signed cert — you grant permissions ONCE:"
  echo "  • If this is the first build since the switch to stable signing, you"
  echo "    need a single re-grant (the old ad-hoc TCC entry no longer matches)."
  echo "  • Open Inspector Rust → Settings tab. The green Restart prompt appears"
  echo "    once you toggle Inspector Rust on in System Settings → Accessibility."
  echo "  • After that, every future rebuild keeps the grant — no re-granting."
  echo "  • Stuck? bash scripts/install-macos.sh --reset (wipes stale TCC entries)."
else
  echo "If Accessibility access is missing after launch:"
  echo "  • Open Inspector Rust → Settings tab. The green Restart prompt appears once"
  echo "    you toggle Inspector Rust on in System Settings → Accessibility."
  echo "  • Or: bash scripts/install-macos.sh --reset (wipes stale TCC entries)."
fi
