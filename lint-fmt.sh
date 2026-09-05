#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Rust files are handled by cargo fmt/clippy.
# C++ source files (headers + translation units) under src/, excluding generated code.
mapfile -t CPP_FILES < <(find src -type f \( -name '*.cpp' -o -name '*.cc' -o -name '*.cxx' \) | sort)
mapfile -t HEADER_FILES < <(find src -type f \( -name '*.h' -o -name '*.hpp' -o -name '*.hh' \) | sort)

echo "== cargo fmt =="
cargo fmt

echo "== clang-format =="
if [[ ${#CPP_FILES[@]} -gt 0 || ${#HEADER_FILES[@]} -gt 0 ]]; then
    clang-format -i "${CPP_FILES[@]}" "${HEADER_FILES[@]}"
else
    echo "no C++ source files found under src/"
fi

echo "== cargo clippy =="
cargo clippy --all-targets --all-features

# --- flags for C++ tooling (clang-tidy / clazy) ---
# Qt flags (includes + -D defines).
QT_FLAGS=$(pkg-config --cflags Qt6Widgets Qt6Core Qt6Gui 2>/dev/null || true)

# cxx-qt generated header roots (best-effort: newest build).
CXXQT_INC=$(ls -dt target/*/build/protonctx-*/out/cxxqtbuild/include 2>/dev/null | head -1)
CXXQT_LIB_INC=$(ls -dt target/*/build/cxx-qt-lib-*/out/cxxqtbuild/include 2>/dev/null | head -1)
CXXQT_CORE_INC=$(ls -dt target/*/build/cxx-qt-[0-9a-f]*/out/cxxqtbuild/include 2>/dev/null | head -1)

# Same version define cxx-qt-build injects via build.rs (from Cargo.toml), so
# clang-tidy/clazy see the same macro as the real build.
PROTONCTX_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

ARGS=()
for f in $QT_FLAGS; do ARGS+=("--extra-arg-before=$f"); done
for d in "$CXXQT_INC" "$CXXQT_LIB_INC" "$CXXQT_CORE_INC"; do
    [[ -n "$d" && -d "$d" ]] && ARGS+=("--extra-arg-before=-I$d")
done
ARGS+=("--extra-arg-before=-std=c++17")
ARGS+=("--extra-arg-before=-DPROTONCTX_VERSION=\"$PROTONCTX_VERSION\"")

if [[ -z "$CXXQT_INC" ]]; then
    echo "warning: could not resolve cxx-qt include roots (run 'cargo build' first); skipping clang-tidy/clazy" >&2
else
    # clang-tidy/clazy need a translation unit (headers are analyzed via the .cpp that includes them).
    if [[ ${#CPP_FILES[@]} -eq 0 ]]; then
        echo "warning: no .cpp files to run clang-tidy/clazy on; skipping" >&2
    else
        echo "== clang-tidy =="
        # Only core/C++ static-analyzer checks (skip clang-analyzer-webkit.*, which
        # fires on Qt system headers), and only report on our own src/ code.
        # No compile_commands.json exists (cargo/cxx-qt build), so clang-tidy prints
        # its "could not auto-detect compilation database" fallback noise to stderr;
        # filter those expected lines and keep only real diagnostics.
        clang-tidy \
            -checks='clang-analyzer-core.*,clang-analyzer-cplusplus.*,clang-analyzer-deadcode.*,clang-analyzer-nullability.*,clang-analyzer-unix.*' \
            --header-filter='^.*/src/.*$' \
            "${ARGS[@]}" "${CPP_FILES[@]}" \
            2>&1 | grep -v "Error while trying to load a compilation database" \
                   | grep -v "Could not auto-detect compilation database for file" \
                   | grep -v "No compilation database found in" \
                   | grep -v "fixed-compilation-database: Error while opening" \
                   | grep -v "json-compilation-database: Error while opening" \
                   | grep -v "Running without flags" || true

        echo "== clazy =="
        # Only report warnings from our own src/ headers, not Qt system headers.
        # Filter the expected "no compilation database" fallback noise (see above).
        clazy-standalone --header-filter='^.*/src/.*$' "${ARGS[@]}" "${CPP_FILES[@]}" \
            2>&1 | grep -v "Error while trying to load a compilation database" \
                   | grep -v "Could not auto-detect compilation database for file" \
                   | grep -v "No compilation database found in" \
                   | grep -v "fixed-compilation-database: Error while opening" \
                   | grep -v "json-compilation-database: Error while opening" \
                   | grep -v "Running without flags" || true
    fi
fi

echo "Done."
