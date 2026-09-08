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
         "$APPDIR/usr/share/icons/hicolor/scalable/apps" \
         "$APPDIR/usr/plugins/styles" \
         "$APPDIR/usr/plugins/platformthemes"

install -m755 target/release/protonctx "$APPDIR/usr/bin/protonctx"
install -m644 packaging/protonctx.desktop \
  "$APPDIR/usr/share/applications/protonctx.desktop"
install -m644 ui/icon/icon.svg \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps/protonctx.svg"

# 3a. Stage the native KDE Plasma theme plugins so Qt's plugin loader finds
#     them in the right subdirectories (styles/ and platformthemes/). The
#     KF6/Qt libraries they depend on are deployed by --library below.
QT_PLUGINS="$(qtpaths6 --plugin-dir)"
install -m644 "$QT_PLUGINS/styles/breeze6.so" \
  "$APPDIR/usr/plugins/styles/breeze6.so"
install -m644 "$QT_PLUGINS/platformthemes/KDEPlasmaPlatformTheme6.so" \
  "$APPDIR/usr/plugins/platformthemes/KDEPlasmaPlatformTheme6.so"

# These plugins are dlopen()'d by Qt at runtime and ship with no RUNPATH, so
# without patching, their KF6/Qt dependencies would resolve to the host's
# /lib64 (newer Qt) instead of the bundled copies — an ABI mismatch that makes
# them fail to load (e.g. "version `Qt_6.11' not found"). Point their deps at
# the bundled usr/lib.
patchelf --set-rpath '$ORIGIN/../../lib' \
  "$APPDIR/usr/plugins/styles/breeze6.so"
patchelf --set-rpath '$ORIGIN/../../lib' \
  "$APPDIR/usr/plugins/platformthemes/KDEPlasmaPlatformTheme6.so"

# 3b. Bundle Qt (libs + QPA platform plugins), the KDE theme plugins and their
#     transitive KF6/Qt dependencies, then emit the AppImage.
#     APPIMAGE_EXTRACT_AND_RUN=1 avoids needing FUSE to run linuxdeploy itself.
export APPIMAGE_EXTRACT_AND_RUN=1

linuxdeploy \
  --appdir "$APPDIR" \
  --executable "$APPDIR/usr/bin/protonctx" \
  --desktop-file "$APPDIR/usr/share/applications/protonctx.desktop" \
  --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/protonctx.svg" \
  --library "$QT_PLUGINS/styles/breeze6.so" \
  --library "$QT_PLUGINS/platformthemes/KDEPlasmaPlatformTheme6.so" \
  --plugin qt \
  --output appimage

echo "Built $(ls protonctx-*.AppImage)"
