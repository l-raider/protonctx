#!/usr/bin/env bash
set -euo pipefail

# Build a protonctx .deb package.
#
# This script is meant to run INSIDE the build container (see
# packaging/Containerfile.deb) so that the binary links against Debian 13's
# oldest-supported glibc/Qt and `dpkg-shlibdeps` populates the runtime
# `Depends:` field. Run from the repo root, e.g.:
#
#   podman run --rm -v "$PWD":/src -w /src protonctx-deb \
#     bash packaging/build-deb-in-container.sh

cd "$(dirname "$0")/.."

# 1. Build the release binary.
cargo build --release

# 2. Render PNG icons (needs rsvg-convert from librsvg2-tools).
./packaging/render-icons.sh

# 3. Package the .deb. --no-build reuses the freshly built binary above.
cargo deb --no-build

echo "Built target/debian/protonctx_*.deb"