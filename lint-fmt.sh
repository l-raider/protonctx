#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

echo "== cargo fmt =="
cargo fmt

echo "== cargo clippy =="
cargo clippy --all-targets --all-features -- -D warnings

echo "Done."
