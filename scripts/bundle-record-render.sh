#!/usr/bin/env bash
#
# bundle-record-render.sh — put the `ir-record-render` helper and its LGPL
# ffmpeg dylibs into an InspectorRust.app, BEFORE the app is signed.
#
#   bash scripts/bundle-record-render.sh <path/to/InspectorRust.app> [rust-target-triple]
#
# Called by install-macos.sh (no triple = host build) and by
# pack-macos-release-dmg.sh (with the release triple).
#
# Layout it produces:
#   Contents/MacOS/ir-record-render                 helper, rpath @executable_path/../Frameworks
#   Contents/Frameworks/libav*.N.dylib, libsw*.N.dylib   install_name @rpath/<name>
#   Contents/Resources/THIRD-PARTY/ffmpeg/          LGPL text, FFmpeg LICENSE.md, SOURCE.txt
#   Contents/Resources/THIRD-PARTY/openscreen/      OpenScreen LICENSE (MIT), notices (Recordly MIT)
#
# Apple Silicon only: the compositor is Metal + VideoToolbox on arm64. For an
# x86_64 triple (the Intel DMG) this script does nothing and exits 0 — the
# editor there reports "helper missing", which is the intended behaviour.
#
# Hard checks (any failure aborts, so a broken bundle never gets signed):
#   - no `otool -L` / LC_RPATH entry of the helper or any bundled dylib points
#     into the repo, third_party/, /opt/homebrew or /usr/local;
#   - every ffmpeg library the helper (transitively) needs is in Frameworks/.
#
# Each dylib and the helper get an ad-hoc signature here (install_name_tool
# invalidates the linker signature, and arm64 refuses to load unsigned code);
# the caller's deep sign re-signs them with the real identity afterwards.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="${1:?usage: $0 <InspectorRust.app> [rust-target-triple]}"
TRIPLE="${2:-}"
FFMPEG_VERSION="8.1.2"
FF_DIR="${REPO_ROOT}/third_party/ffmpeg/${FFMPEG_VERSION}"
HELPER="ir-record-render"

die() { echo "✘ bundle-record-render: $*" >&2; exit 1; }

arch="${TRIPLE%%-*}"
[[ -n "${arch}" ]] || arch="$(uname -m)"
case "${arch}" in
  aarch64|arm64) ;;
  *)
    echo "▸ record editor helper: skipped for ${arch} (Apple Silicon only)"
    exit 0
    ;;
esac
[[ "$(uname -m)" == "arm64" ]] || die "the helper is built on Apple Silicon only"
[[ -d "${APP}/Contents/MacOS" ]] || die "not an app bundle: ${APP}"

echo "▸ Building LGPL ffmpeg ${FFMPEG_VERSION} (skipped when already built)…"
bash "${REPO_ROOT}/scripts/build-ffmpeg-macos.sh"

echo "▸ Building the ${HELPER} helper…"
target_args=()
bin_dir="${REPO_ROOT}/target/release"
if [[ -n "${TRIPLE}" ]]; then
  target_args=(--target "${TRIPLE}")
  bin_dir="${REPO_ROOT}/target/${TRIPLE}/release"
fi
env -u CARGO_TARGET_DIR MAC_FFMPEG_DIR="${FF_DIR}" CARGO_INCREMENTAL=0 \
  cargo build --release ${target_args[@]+"${target_args[@]}"} \
  -p record-render --bin "${HELPER}" --manifest-path "${REPO_ROOT}/Cargo.toml"

MACOS_DIR="${APP}/Contents/MacOS"
FW_DIR="${APP}/Contents/Frameworks"
mkdir -p "${FW_DIR}"
cp "${bin_dir}/${HELPER}" "${MACOS_DIR}/${HELPER}"
chmod u+w "${MACOS_DIR}/${HELPER}"
xattr -c "${MACOS_DIR}/${HELPER}" 2>/dev/null || true

# The ffmpeg references of one Mach-O, as absolute paths inside FF_DIR.
ff_refs() {
  otool -L "$1" | tail -n +2 | awk '{print $1}' | grep -F "${FF_DIR}/lib/" || true
}

# Copy the transitive closure of ffmpeg dylibs the helper needs (by the
# install name they are referenced with, e.g. libavcodec.62.dylib).
queue=("${MACOS_DIR}/${HELPER}")
copied=()
while ((${#queue[@]})); do
  cur="${queue[0]}"
  queue=("${queue[@]:1}")
  while IFS= read -r ref; do
    [[ -n "${ref}" ]] || continue
    name="$(basename "${ref}")"
    if [[ ! -f "${FW_DIR}/${name}" ]]; then
      [[ -e "${ref}" ]] || die "missing ffmpeg library ${ref}"
      cp -L "${ref}" "${FW_DIR}/${name}"
      chmod u+w "${FW_DIR}/${name}"
      xattr -c "${FW_DIR}/${name}" 2>/dev/null || true
      copied+=("${name}")
      queue+=("${FW_DIR}/${name}")
    fi
  done < <(ff_refs "${cur}")
done
((${#copied[@]})) || die "the helper links no ffmpeg library — built against an empty compositor?"

# Rewrite every ffmpeg reference to @rpath/<name>.
rewrite() {
  local file="$1" ref
  while IFS= read -r ref; do
    [[ -n "${ref}" ]] || continue
    install_name_tool -change "${ref}" "@rpath/$(basename "${ref}")" "${file}"
  done < <(ff_refs "${file}")
}
for name in "${copied[@]}"; do
  install_name_tool -id "@rpath/${name}" "${FW_DIR}/${name}"
  rewrite "${FW_DIR}/${name}"
done
rewrite "${MACOS_DIR}/${HELPER}"
# Drop any build-time rpath (e.g. a cargo-added link dir), then add the bundle one.
while IFS= read -r rp; do
  [[ -n "${rp}" ]] || continue
  install_name_tool -delete_rpath "${rp}" "${MACOS_DIR}/${HELPER}"
done < <(otool -l "${MACOS_DIR}/${HELPER}" | awk '/LC_RPATH/{f=1} f&&/ path /{print $2; f=0}')
install_name_tool -add_rpath "@executable_path/../Frameworks" "${MACOS_DIR}/${HELPER}"

# Check: nothing may point back into the build machine.
bad_pattern="${REPO_ROOT}|/third_party/|/opt/homebrew|/usr/local"
for f in "${MACOS_DIR}/${HELPER}" "${FW_DIR}"/*.dylib; do
  links="$(otool -L "${f}" | tail -n +2; otool -l "${f}" | awk '/LC_RPATH/{r=1} r&&/ path /{print; r=0}')"
  if grep -E "${bad_pattern}" <<<"${links}" >/dev/null; then
    echo "${links}" | grep -E "${bad_pattern}" >&2
    die "$(basename "${f}") still references a build-machine path"
  fi
done

# Ad-hoc signatures (see header); the caller's deep sign replaces them.
for f in "${FW_DIR}"/*.dylib "${MACOS_DIR}/${HELPER}"; do
  codesign --force --sign - "${f}" >/dev/null
done

# Licence notices.
TP="${APP}/Contents/Resources/THIRD-PARTY"
rm -rf "${TP}"
mkdir -p "${TP}/ffmpeg" "${TP}/openscreen"
cp "${REPO_ROOT}/vendor/ffmpeg-licence/COPYING.LGPLv2.1" "${TP}/ffmpeg/COPYING.LGPLv2.1"
cp "${REPO_ROOT}/vendor/ffmpeg-licence/LICENSE.md" "${TP}/ffmpeg/LICENSE.md"
stamp="${FF_DIR}/.ir-ffmpeg-stamp"
{
  echo "FFmpeg ${FFMPEG_VERSION} — shared libraries in Contents/Frameworks/"
  echo
  echo "License: GNU Lesser General Public License v2.1 or later (COPYING.LGPLv2.1)."
  echo "Built WITHOUT --enable-gpl and WITHOUT --enable-nonfree; no GPL-only library is linked."
  echo
  echo "Corresponding source: https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
  echo "Build script: https://github.com/pepperonas/inspector-rust/blob/main/scripts/build-ffmpeg-macos.sh"
  echo
  echo "Exact build (version, tarball sha256, configure flags):"
  sed 's/^/  /' "${stamp}"
  echo
  echo "Relinking: the libraries are dynamic. You may replace them with your own"
  echo "build of the same FFmpeg version (same install names, @rpath/<name>)."
} >"${TP}/ffmpeg/SOURCE.txt"
cp "${REPO_ROOT}/vendor/openscreen-compositor/LICENSE" "${TP}/openscreen/LICENSE"
cp "${REPO_ROOT}/vendor/openscreen-compositor/THIRD-PARTY-NOTICES.md" "${TP}/openscreen/THIRD-PARTY-NOTICES.md"
cp "${REPO_ROOT}/vendor/openscreen-compositor/VENDORED.md" "${TP}/openscreen/VENDORED.md"

echo "▸ Bundled ${HELPER} + ${#copied[@]} ffmpeg dylibs (${copied[*]})"
