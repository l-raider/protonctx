#!/usr/bin/env bash
set -euo pipefail

# Build a protonctx .deb from scratch: builds the build container, then runs
# the actual .deb build inside it (so the binary links against Debian 13's
# glibc/Qt — the oldest we must support, and dpkg-shlibdeps fills in the Qt6
# runtime dependencies).
#
# Usage (from the project root):
#   ./build-deb.sh

cd "$(dirname "$0")"

# 1. Build the image (once).
podman build -t protonctx-deb -f packaging/Containerfile.deb .

# 2. Produce the .deb.
#    :Z relabels the bind mount so the container can read/write it under
#    SELinux (Fedora/RHEL). Required on enforcing SELinux hosts.
podman run --rm -v "$PWD":/src:Z -w /src protonctx-deb \
  bash packaging/build-deb-in-container.sh
