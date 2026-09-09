#!/usr/bin/env bash
# Build a Flatpak for protonctx.
#
# First run:  chmod +x build-flatpak.sh
# Then:       ./build-flatpak.sh
#
# Re-run any time you change Cargo.lock or source code.
# The vendor directory is regenerated automatically when Cargo.lock changes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

MANIFEST="flatpak/io.github.l_raider.protonctx.yml"
BUILD_DIR="flatpak/build"
REPO_DIR="flatpak/repo"
VENDOR_DIR="flatpak/vendor"
VENDOR_CONFIG="flatpak/cargo-vendor-config.toml"
VERIFY_BEFORE_BUILD="${VERIFY_BEFORE_BUILD:-1}"

# ── Dependency checks ────────────────────────────────────────────────────────
for cmd in flatpak flatpak-builder cargo rsvg-convert; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "Error: '$cmd' is not installed. Please install it and try again." >&2
        echo "  rsvg-convert is provided by: sudo apt install librsvg2-bin" >&2
        exit 1
    fi
done

if [[ "$VERIFY_BEFORE_BUILD" != "0" ]]; then
    echo "Running cargo test..."
    cargo test
else
    echo "Skipping cargo test (VERIFY_BEFORE_BUILD=0)..."
fi

# ── Vendor all crates when Cargo.lock is newer than the vendor dir ───────────
if [[ ! -d "$VENDOR_DIR" || Cargo.lock -nt "$VENDOR_DIR" ]]; then
    echo "Vendoring crates (cargo vendor)..."
    cargo vendor "$VENDOR_DIR" > "$VENDOR_CONFIG"
    touch "$VENDOR_DIR"   # update mtime so we don't re-vendor unnecessarily
fi

# ── Generate icon PNGs from SVG when the SVG is newer ────────────────────────
SVG_ICON="ui/icon/icon.svg"
ICON_SENTINEL="flatpak/icons/hicolor/256x256/apps/io.github.l_raider.protonctx.png"
if [[ ! -f "$ICON_SENTINEL" || "$SVG_ICON" -nt "$ICON_SENTINEL" ]]; then
    echo "Generating icon PNGs from SVG..."
    for size in 16 32 48 64 128 256 512; do
        dir="flatpak/icons/hicolor/${size}x${size}/apps"
        mkdir -p "$dir"
        rsvg-convert -w "$size" -h "$size" "$SVG_ICON" \
            -o "$dir/io.github.l_raider.protonctx.png"
    done
    mkdir -p "flatpak/icons/hicolor/scalable/apps"
    cp "$SVG_ICON" "flatpak/icons/hicolor/scalable/apps/io.github.l_raider.protonctx.svg"
fi

# ── Ensure flathub remote is present (needed to fetch SDKs) ──────────────────
if ! flatpak remote-list --user | grep -q '^flathub'; then
    echo "Adding flathub remote..."
    flatpak remote-add --user --if-not-exists flathub \
        https://dl.flathub.org/repo/flathub.flatpakrepo
fi

# ── Ensure the KDE SDK is installed (declared in the manifest) ───────────────
# The rust-stable branch is derived from the KDE SDK metadata below, so the SDK
# must be present first. flatpak-builder would install it anyway via
# --install-deps-from=flathub, but we need it here to resolve the branch.
KDE_BRANCH=$(awk '/^runtime-version:/{gsub(/[^0-9.]/, "", $2); print $2; exit}' "$MANIFEST")
KDE_BRANCH="${KDE_BRANCH:-6.10}"
KDE_SDK="org.kde.Sdk/x86_64/${KDE_BRANCH}"
if ! flatpak list --runtime --columns=ref --all 2>/dev/null | grep -qF "$KDE_SDK"; then
    echo "Installing ${KDE_SDK}..."
    flatpak install --user -y flathub "$KDE_SDK"
fi

# ── Ensure the rust-stable SDK extension is installed ────────────────────────
# Look up the freedesktop branch from the KDE SDK metadata to find the matching
# rust-stable branch. Guard the lookup so a missing/empty metadata can't abort
# the script under `set -euo pipefail`; fall back to the default branch.
FD_BRANCH=$(flatpak info --show-metadata "$KDE_SDK" 2>/dev/null \
    | awk '/^\[Extension org\.freedesktop\.Platform\.GL\]/{f=1}
           f && /^versions=/{split($0,a,"[=;]"); print a[2]; exit}' \
    || true)
FD_BRANCH="${FD_BRANCH:-25.08}"
RUST_EXT="org.freedesktop.Sdk.Extension.rust-stable/x86_64/${FD_BRANCH}"
if ! flatpak list --runtime --columns=ref --all 2>/dev/null | grep -qF "$RUST_EXT"; then
    echo "Installing ${RUST_EXT}..."
    flatpak install --user -y flathub "$RUST_EXT"
fi

# ── Build ─────────────────────────────────────────────────────────────────────
echo "Running flatpak-builder..."
flatpak-builder \
    --force-clean \
    --disable-rofiles-fuse \
    --user \
    --install-deps-from=flathub \
    --repo="$REPO_DIR" \
    "$BUILD_DIR" \
    "$MANIFEST"

# ── Create the distributable .flatpak bundle ──────────────────────────────────
# Name the bundle with the version from Cargo.toml, e.g. protonctx-v1.1.0.flatpak
VERSION=$(awk -F' = ' '/^version =/{gsub(/"/, "", $2); print $2; exit}' Cargo.toml)
VERSION="${VERSION:-0.0.0}"
BUNDLE="protonctx-v${VERSION}.flatpak"
echo "Creating ${BUNDLE}..."
flatpak build-bundle "$REPO_DIR" "$BUNDLE" io.github.l_raider.protonctx

echo
echo "Done! Bundle created: ${BUNDLE}"
echo
echo "  Test without installing:"
echo "    flatpak-builder --run $BUILD_DIR $MANIFEST protonctx"
echo
echo "  Install from bundle:"
echo "    flatpak install --user ${BUNDLE}"
echo "    flatpak run io.github.l_raider.protonctx"
