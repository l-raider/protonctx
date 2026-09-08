#!/usr/bin/env bash
set -euo pipefail

# Build a protonctx AppImage from scratch: builds the build container, then
# runs the actual AppImage build inside it (so the binary links against
# Debian 13's glibc — the oldest we must support).
#
# Usage (from the project root):
#   ./build-appimage.sh

cd "$(dirname "$0")"

# 1. Build the image (once).
podman build -t protonctx-appimage -f packaging/Containerfile.appimage .

# 2. Produce the AppImage.
#    :Z relabels the bind mount so the container can read/write it under
#    SELinux (Fedora/RHEL). Required on enforcing SELinux hosts.
podman run --rm -v "$PWD":/src:Z -w /src protonctx-appimage \
  bash packaging/build-appimage-in-container.sh
