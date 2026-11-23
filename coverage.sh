#!/usr/bin/env bash
# Measure test coverage per module.
#
# cargo-llvm-cov on nightly gives line and branch coverage (branch coverage
# is nightly-only); on stable, line coverage only. Without cargo-llvm-cov,
# cargo-tarpaulin is used (line coverage).
#
# Extra arguments go to the coverage tool, e.g. `./coverage.sh --html`.
set -euo pipefail
cd "$(dirname "$0")"

if cargo llvm-cov --version >/dev/null 2>&1; then
    if rustup run nightly rustc --version >/dev/null 2>&1; then
        echo "== cargo +nightly llvm-cov --branch =="
        # Its own target dir: profiles left by a stable run in the shared one
        # skew the nightly report.
        CARGO_LLVM_COV_TARGET_DIR="$PWD/target/llvm-cov-nightly" \
            cargo +nightly llvm-cov --branch "$@"
    else
        echo "== cargo llvm-cov (no nightly toolchain: line coverage only) =="
        cargo llvm-cov "$@"
    fi
elif cargo tarpaulin --version >/dev/null 2>&1; then
    echo "== cargo tarpaulin =="
    cargo tarpaulin --out Stdout "$@"
else
    echo "Install cargo-llvm-cov (preferred) or cargo-tarpaulin." >&2
    exit 1
fi
