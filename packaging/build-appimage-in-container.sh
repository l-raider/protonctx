#!/usr/bin/env bash
set -euo pipefail

# Build a protonctx AppImage.
#
# This script is meant to run INSIDE the build container (see
# packaging/Containerfile.appimage) so that the binary links against the
# oldest glibc we must support. Run from the repo root, e.g.:
#
#   podman run --rm -v "$PWD":/src -w /src protonctx-appimage \
#     bash packaging/build-appimage.sh

cd "$(dirname "$0")/.."

# 1. Build the release binary.
cargo build --release

# 2. Assemble the AppDir.
APPDIR="$(pwd)/AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/scalable/apps"

install -m755 target/release/protonctx "$APPDIR/usr/bin/protonctx"
install -m644 packaging/protonctx.desktop \
  "$APPDIR/usr/share/applications/protonctx.desktop"
install -m644 ui/icon/icon.svg \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps/protonctx.svg"

# 3. Bundle Qt (libs + QPA platform plugins) and emit the AppImage.
#    APPIMAGE_EXTRACT_AND_RUN=1 avoids needing FUSE to run linuxdeploy itself.
export APPIMAGE_EXTRACT_AND_RUN=1

linuxdeploy \
  --appdir "$APPDIR" \
  --executable "$APPDIR/usr/bin/protonctx" \
  --desktop-file "$APPDIR/usr/share/applications/protonctx.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/protonctx.svg" \
  --plugin qt \
  --output appimage

echo "Built $(ls protonctx-*.AppImage)"
