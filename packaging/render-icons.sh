#!/usr/bin/env bash
# Render PNG icons (16–256 px) from the scalable SVG master in
# packaging/icons/hicolor for use by cargo-deb / cargo-generate-rpm.
set -euo pipefail

cd "$(dirname "$0")"

SVG="icons/hicolor/scalable/apps/protonctx.svg"
OUT="icons/hicolor"

if command -v rsvg-convert >/dev/null 2>&1; then
    render() { rsvg-convert -w "$1" -h "$1" "$SVG" -o "$2"; }
elif command -v convert >/dev/null 2>&1; then
    render() { convert -background none "$SVG" -resize "${1}x${1}" "$2"; }
else
    echo "error: need rsvg-convert (librsvg2-tools) or ImageMagick convert to render icons" >&2
    exit 1
fi

for size in 16 24 32 48 64 128 256; do
    dir="$OUT/${size}x${size}/apps"
    mkdir -p "$dir"
    render "$size" "$dir/protonctx.png"
done

echo "rendered icons into $OUT"
