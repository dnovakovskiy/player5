#!/usr/bin/env bash
# Builds core/ffi as a static library for every Apple platform the Swift
# shells run on and packages it, with its C header and a module map, as
#
#   apps/mac/Frameworks/Player5Core.xcframework   (generated, gitignored)
#
# Consumers (ADR-0008):
#   - apps/mac/Package.swift      SwiftPM binaryTarget `Player5Core`
#   - apps/mac/project.yml        XcodeGen macOS app (links it directly)
#   - apps/ios/project.yml        XcodeGen iOS app (through the package)
#
# Slices: macOS (arm64 + x86_64), iOS (arm64), iOS Simulator (arm64 + x86_64).
#
# Needs macOS with Xcode command-line tools and rustup. Installs cbindgen
# with `cargo install --locked cbindgen` if it is missing (a build tool,
# never a dependency of the crates). Written for bash 3.2, the macOS
# system bash: no associative arrays, no mapfile, no ${var,,}.
#
# Usage: scripts/build-xcframework.sh
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

MAC_TARGETS="aarch64-apple-darwin x86_64-apple-darwin"
IOS_TARGETS="aarch64-apple-ios"
SIM_TARGETS="aarch64-apple-ios-sim x86_64-apple-ios"
ALL_TARGETS="$MAC_TARGETS $IOS_TARGETS $SIM_TARGETS"

LIB_NAME="libplayer5.a"
MODULE_NAME="Player5Core"
WORK="$ROOT/target/xcframework"
DEST="$ROOT/apps/mac/Frameworks/$MODULE_NAME.xcframework"

# Object files must not claim a newer OS than the Swift packages target
# (macOS 13, iOS 16), or the Apple linker warns on every object.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"

log() { printf '\n==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || die "this script runs on macOS (it needs Xcode's lipo and xcodebuild)"
command -v xcodebuild >/dev/null 2>&1 || die "xcodebuild not found; install Xcode and run xcode-select"
command -v lipo >/dev/null 2>&1 || die "lipo not found; install the Xcode command-line tools"
command -v cargo >/dev/null 2>&1 || die "cargo not found; install Rust with rustup"
command -v rustup >/dev/null 2>&1 || die "rustup not found; the Apple targets are installed with rustup"

log "Rust targets"
# shellcheck disable=SC2086 # word splitting of the target lists is intended
rustup target add $ALL_TARGETS

log "Static libraries"
# core/ffi lists staticlib, cdylib and rlib; asking `cargo rustc` for the
# staticlib alone avoids linking a dylib for every target.
for target in $ALL_TARGETS; do
    echo "--- $target"
    cargo rustc -p player5-ffi --release --target "$target" --crate-type staticlib
    [ -f "target/$target/release/$LIB_NAME" ] || die "missing target/$target/release/$LIB_NAME"
done

rm -rf "$WORK"
mkdir -p "$WORK/macos" "$WORK/ios" "$WORK/ios-simulator" "$WORK/headers"

log "Universal slices"
lipo -create \
    "target/aarch64-apple-darwin/release/$LIB_NAME" \
    "target/x86_64-apple-darwin/release/$LIB_NAME" \
    -output "$WORK/macos/$LIB_NAME"
cp "target/aarch64-apple-ios/release/$LIB_NAME" "$WORK/ios/$LIB_NAME"
lipo -create \
    "target/aarch64-apple-ios-sim/release/$LIB_NAME" \
    "target/x86_64-apple-ios/release/$LIB_NAME" \
    -output "$WORK/ios-simulator/$LIB_NAME"
lipo -info "$WORK/macos/$LIB_NAME" "$WORK/ios/$LIB_NAME" "$WORK/ios-simulator/$LIB_NAME"

log "C header (cbindgen)"
CBINDGEN="cbindgen"
if ! command -v cbindgen >/dev/null 2>&1; then
    CBINDGEN="${CARGO_HOME:-$HOME/.cargo}/bin/cbindgen"
    if [ ! -x "$CBINDGEN" ]; then
        echo "cbindgen not found; installing it (cargo install --locked cbindgen)"
        cargo install --locked cbindgen
    fi
fi
"$CBINDGEN" --version

# Start from core/ffi/cbindgen.toml, minus any `prefix = ...` keys. The
# exported Rust names already carry their prefixes (P5Control, p5_split_new);
# cbindgen's [export] prefix would rename the types to P5P5Control and its
# [fn] prefix is a literal token placed before every declaration, which
# makes the header invalid C. Without those keys this is a no-op filter.
sed -e '/^[[:space:]]*prefix[[:space:]]*=/d' core/ffi/cbindgen.toml > "$WORK/cbindgen.toml"
"$CBINDGEN" --config "$WORK/cbindgen.toml" --crate player5-ffi --output "$WORK/headers/player5.h"

cat > "$WORK/headers/module.modulemap" <<EOF
module $MODULE_NAME {
    header "player5.h"
    export *
}
EOF

# Fail here, with a readable error, rather than deep inside Swift's importer.
grep -q 'p5_split_new' "$WORK/headers/player5.h" || die "header lacks p5_split_new"
grep -q 'typedef struct P5Control P5Control;' "$WORK/headers/player5.h" \
    || die "header does not declare the opaque P5Control type"
xcrun clang -fsyntax-only -x c "$WORK/headers/player5.h" \
    || die "generated header is not valid C"

log "XCFramework"
rm -rf "$DEST"
mkdir -p "$(dirname "$DEST")"
xcodebuild -create-xcframework \
    -library "$WORK/macos/$LIB_NAME" -headers "$WORK/headers" \
    -library "$WORK/ios/$LIB_NAME" -headers "$WORK/headers" \
    -library "$WORK/ios-simulator/$LIB_NAME" -headers "$WORK/headers" \
    -output "$DEST"

log "Done"
find "$DEST" -maxdepth 3 -print | sed "s|^$ROOT/||"
