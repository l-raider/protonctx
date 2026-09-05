#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cargo build --release
strip -s target/release/protonctx
./packaging/render-icons.sh

DIST="$(rpm --eval '%{dist}' 2>/dev/null || true)"
# On hosts without the %dist macro defined (e.g. non-RPM distros), rpm
# echoes the literal "%{dist}" back instead of failing or returning empty.
if [[ "${DIST}" == '%{dist}' ]]; then
	DIST=""
fi
RELEASE="1${DIST}"

cargo generate-rpm -s "release = \"${RELEASE}\""

echo "Built target/release/protonctx"
echo "Built target/generate-rpm/protonctx-*.rpm"
