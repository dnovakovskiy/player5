#!/usr/bin/env sh
# Builds core/ffi for the browser and drops the module where the web app
# serves it from. Requires: rustup target add wasm32-unknown-unknown
#
# The pure-JavaScript copy of the core (the fallback for pages whose CSP
# forbids WebAssembly, ADR-0010) is derived from this file by
# apps/web/scripts/gen-core.mjs; `npm run verify-wasm`, `npm run build` and
# `npm run build:single` regenerate it automatically when this file is newer.
set -eu
cd "$(dirname "$0")/.."
cargo build -p player5-ffi --target wasm32-unknown-unknown --profile wasm
mkdir -p apps/web/public
cp target/wasm32-unknown-unknown/wasm/player5.wasm apps/web/public/player5.wasm
ls -l apps/web/public/player5.wasm
