#!/usr/bin/env bash
# Build the "boom Audio" virtual audio driver (AudioServerPlugIn).
#
# The driver source is BlackHole (MIT, vendored verbatim as BlackHole.c) —
# rebranded to "boom Audio" purely via -D compile defines (no source edits), so
# it never collides with a separately-installed BlackHole (own device UID,
# bundle id, and a fresh CFPlugIn factory UUID in Info.plist).
#
# Output: build/boom-driver.driver — a universal (arm64+x86_64) .driver bundle,
# ad-hoc signed. **For distribution to other Macs it must be re-signed with a
# Developer ID and notarized** (ad-hoc is only trusted to load locally).
set -euo pipefail
cd "$(dirname "$0")"

BUNDLE="build/boom-driver.driver"
rm -rf build
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"

clang -bundle -O2 -mmacosx-version-min=11.0 \
  -arch arm64 -arch x86_64 \
  -Wno-deprecated-declarations \
  -DkDriver_Name='"BoomAudio"' \
  -DkHas_Driver_Name_Format=0 \
  -DkDevice_Name='"boom Audio"' \
  -DkDevice2_Name='"boom Audio Mirror"' \
  -DkPlugIn_BundleID='"io.celox.boom.driver"' \
  -DkManufacturer_Name='"celox"' \
  -DkNumber_Of_Channels=2 \
  -framework CoreFoundation -framework CoreAudio -framework Accelerate \
  -o "$BUNDLE/Contents/MacOS/boom-audio" \
  BlackHole.c

cp Info.plist "$BUNDLE/Contents/Info.plist"

# Ad-hoc sign (local trust). Distribution: replace `-` with "Developer ID
# Application: <name>" + notarize the bundle.
codesign --force --deep --sign - "$BUNDLE"

echo "✓ built $BUNDLE  (device UID: BoomAudio_UID, name: \"boom Audio\")"
echo "  install with: scripts/boom-driver-install.sh"
