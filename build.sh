#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cargo build --profile publicrelease
echo "build done..."
