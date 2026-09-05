#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cargo build --release
./packaging/render-icons.sh
cargo deb --no-build

echo "Built target/debian/protonctx_*.deb"
