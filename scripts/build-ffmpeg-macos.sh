#!/usr/bin/env bash
# Builds an LGPL, shared ffmpeg 8.1.2 from source for the `record` compositor
# (vendor/openscreen-compositor, later the ir-record-render helper).
#
#   scripts/build-ffmpeg-macos.sh            → third_party/ffmpeg/8.1.2/{include,lib,bin}
#   MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2 cargo test -p openscreen-compositor --lib
#
# Licence: NEVER --enable-gpl / --enable-nonfree. Prebuilt macOS ffmpeg binaries
# (Homebrew, evermeet, …) are GPL (x264/x265) and must not be linked into the app.
# The result is verified by the binary's own licence banner (`ffmpeg -L`) and by
# config.h, not by trusting the configure line.
#
# Idempotent: a tree whose stamp file matches version + tarball hash + flags is
# left alone. Any other existing tree is rebuilt from scratch.
#
# Tarball hash: taken verbatim from OpenScreen's scripts/fetch-ffmpeg-macos.mjs
# (v2.1.2, commit bfd5bc32), which notes that Debian's .orig.tar.xz is
# byte-identical (used here as fallback mirror only; the hash gates both).
set -euo pipefail

VERSION="8.1.2"
TARBALL_SHA256="464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c"
MACOS_MIN="13.0"
URLS=(
  "https://ffmpeg.org/releases/ffmpeg-${VERSION}.tar.xz"
  "https://deb.debian.org/debian/pool/main/f/ffmpeg/ffmpeg_${VERSION}.orig.tar.xz"
)
CONFIGURE_FLAGS=(
  --enable-shared
  --disable-static
  --disable-doc
  --disable-debug
  # Auto-detected from the build machine's Homebrew otherwise → absolute
  # /opt/homebrew paths inside the dylibs, unresolvable on a user's Mac.
  --disable-xlib
  --disable-libxcb
  --disable-sdl2
  --disable-lzma
  --enable-videotoolbox
  --enable-audiotoolbox
  --disable-x86asm
  --arch=arm64
  --cc=clang
  "--extra-cflags=-mmacosx-version-min=${MACOS_MIN}"
  "--extra-ldflags=-mmacosx-version-min=${MACOS_MIN}"
)

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/third_party/ffmpeg/${VERSION}"
WORK="${ROOT}/third_party/ffmpeg/.build-${VERSION}"
STAMP="${DEST}/.ir-ffmpeg-stamp"

die() { echo "build-ffmpeg-macos: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "macOS only"
[[ "$(uname -m)" == "arm64" ]] || die "Apple Silicon only (the record compositor ships for aarch64)"
for f in "${CONFIGURE_FLAGS[@]}"; do
  case "$f" in --enable-gpl|--enable-nonfree|--enable-version3) die "refusing licence-changing flag $f";; esac
done

expected_stamp() {
  printf 'version=%s\nsha256=%s\nflags=%s\n' "$VERSION" "$TARBALL_SHA256" "${CONFIGURE_FLAGS[*]}"
}

is_lgpl() {
  local bin="$DEST/bin/ffmpeg" banner
  [[ -x "$bin" ]] || return 1
  banner="$("$bin" -hide_banner -L 2>&1)" || return 1
  grep -qi 'Lesser General Public' <<<"$banner" || return 1
  ! grep -qi 'GNU General Public License' <<<"$banner"
}

config_h_clean() {
  # config.h carries the configure line; it must not mention gpl/nonfree.
  local cfg="$1"
  [[ -f "$cfg" ]] || return 1
  [[ "$(grep -c -- 'enable-gpl' "$cfg" || true)" == "0" ]] || return 1
  [[ "$(grep -c -- 'enable-nonfree' "$cfg" || true)" == "0" ]] || return 1
  grep -q '#define CONFIG_GPL 0' "$cfg" && grep -q '#define CONFIG_NONFREE 0' "$cfg"
}

if [[ -d "$DEST/include" && -d "$DEST/lib" && -f "$STAMP" ]] \
   && [[ "$(cat "$STAMP")" == "$(expected_stamp)" ]] && is_lgpl; then
  echo "ffmpeg ${VERSION} already built at ${DEST} (LGPL, stamp matches). Nothing to do."
  exit 0
fi

if [[ -e "$DEST" ]]; then
  echo "Existing ${DEST} is stale or unverified — rebuilding."
  rm -rf "$DEST"
fi
rm -rf "$WORK"
mkdir -p "$WORK"
trap 'rm -rf "$WORK"' EXIT

TARBALL="$WORK/ffmpeg-${VERSION}.tar.xz"
ok=0
for url in "${URLS[@]}"; do
  echo "Downloading ffmpeg ${VERSION} from ${url}"
  if curl -fsSL --connect-timeout 20 --speed-limit 1024 --speed-time 30 \
      --retry 3 --retry-delay 2 --retry-all-errors -o "$TARBALL" "$url"; then
    actual="$(shasum -a 256 "$TARBALL" | awk '{print $1}')"
    if [[ "$actual" == "$TARBALL_SHA256" ]]; then
      echo "Checksum OK ($actual)."
      ok=1
      break
    fi
    echo "  checksum mismatch: $actual" >&2
  else
    echo "  download failed" >&2
  fi
done
[[ $ok == 1 ]] || die "could not obtain a tarball matching sha256 ${TARBALL_SHA256}"

tar -xJf "$TARBALL" -C "$WORK"
SRC="$WORK/ffmpeg-${VERSION}"

echo "Configuring (LGPL, shared, macOS ${MACOS_MIN})…"
( cd "$SRC" && ./configure --prefix="$DEST" "${CONFIGURE_FLAGS[@]}" )

config_h_clean "$SRC/config.h" || die "config.h reports a GPL/nonfree configuration"

echo "Building…"
make -C "$SRC" -j "$(sysctl -n hw.ncpu)"
make -C "$SRC" install

# Keep a copy of config.h next to the tree: it is the configure-line evidence
# for the LGPL claim after the build folder is gone.
cp "$SRC/config.h" "$DEST/config.h"

if ! is_lgpl; then
  rm -rf "$DEST"
  die "freshly built ffmpeg does not report an LGPL licence; tree removed"
fi
if otool -L "$DEST"/lib/*.dylib | grep -E '/(opt/homebrew|usr/local)/' >/dev/null; then
  rm -rf "$DEST"
  die "dylibs link against Homebrew/usr/local libraries; tree removed"
fi

expected_stamp >"$STAMP"
echo "Built LGPL ffmpeg ${VERSION} at ${DEST}"
