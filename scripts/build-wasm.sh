#!/usr/bin/env sh
# Builds core/ffi for the browser and drops the module where the web app
# serves it from. Requires: rustup target add wasm32-unknown-unknown
set -eu
cd "$(dirname "$0")/.."
cargo build -p player5-ffi --target wasm32-unknown-unknown --profile wasm
mkdir -p apps/web/public
cp target/wasm32-unknown-unknown/wasm/player5.wasm apps/web/public/player5.wasm
ls -l apps/web/public/player5.wasm
