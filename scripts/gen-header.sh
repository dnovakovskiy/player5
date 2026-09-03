#!/usr/bin/env sh
# Regenerates the C header for the Swift shells from core/ffi.
# Requires: cargo install cbindgen
set -eu
cd "$(dirname "$0")/.."
mkdir -p core/ffi/include
cbindgen --config core/ffi/cbindgen.toml --crate player5-ffi --output core/ffi/include/player5.h
echo "wrote core/ffi/include/player5.h"
