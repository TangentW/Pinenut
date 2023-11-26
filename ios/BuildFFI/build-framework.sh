#!/usr/bin/env bash

FRAMEWORK_NAME="PinenutFFI"
BUILD_PROFILE="release"

WORKING_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
REPO_ROOT="$( dirname "$( dirname "$WORKING_DIR" )" )"
MANIFEST_PATH="$REPO_ROOT/pinenut-ffi/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  echo "Could not locate Cargo.toml in $MANIFEST_PATH"
  exit 1
fi

CRATE_NAME=$(grep --max-count=1 '^name =' "$MANIFEST_PATH" | cut -d '"' -f 2)
if [[ -z "$CRATE_NAME" ]]; then
  echo "Could not determine crate name from $MANIFEST_PATH"
  exit 1
fi

LIB_NAME="libpinenut_ffi.a"

DEFAULT_RUSTFLAGS=""
BUILD_ARGS=(build --manifest-path "$MANIFEST_PATH" --lib)
case $BUILD_PROFILE in
  debug) ;;
  release)
    BUILD_ARGS=("${BUILD_ARGS[@]}" --release)
    # With debuginfo, the zipped artifact quickly baloons to many
    # hundred megabytes in size. Ideally we'd find a way to keep
    # the debug info but in a separate artifact.
    DEFAULT_RUSTFLAGS="-C debuginfo=0"
    ;;
  *) echo "Unknown build profile: $BUILD_PROFILE"; exit 1;
esac

CARGO="$HOME/.cargo/bin/cargo"

cargo_build () {
  TARGET=$1
  if [[ $TARGET == aarch64* ]]; then
    DEFAULT_RUSTFLAGS="${DEFAULT_RUSTFLAGS} --cfg aes_armv8"
  fi

  env -i \
    PATH="${PATH}" \
    RUSTC_WRAPPER="${RUSTC_WRAPPER:-}" \
    RUST_LOG="${RUST_LOG:-}" \
    RUSTFLAGS="${RUSTFLAGS:-$DEFAULT_RUSTFLAGS}" \
    "$CARGO" "${BUILD_ARGS[@]}" --target "$TARGET"
}

set -euvx

# Intel iOS simulator
CFLAGS_x86_64_apple_ios="-target x86_64-apple-ios" \
  cargo_build x86_64-apple-ios

# Hardware iOS targets
cargo_build aarch64-apple-ios

# M1 iOS simulator.
CFLAGS_aarch64_apple_ios_sim="--target aarch64-apple-ios-sim" \
  cargo_build aarch64-apple-ios-sim

TARGET_DIR="$REPO_ROOT/target"
XCFRAMEWORK_ROOT="$( dirname "$WORKING_DIR" )/$FRAMEWORK_NAME.xcframework"

# Start from a clean slate.

rm -rf "$XCFRAMEWORK_ROOT"

# Build the directory structure right for an individual framework.
# Most of this doesn't change between architectures.

COMMON="$XCFRAMEWORK_ROOT/common/$FRAMEWORK_NAME.framework"

mkdir -p "$COMMON/Modules"
cp "$WORKING_DIR/module.modulemap" "$COMMON/Modules/"

mkdir -p "$COMMON/Headers"
cbindgen "$REPO_ROOT/pinenut-ffi" -l C -o "$COMMON/Headers/$FRAMEWORK_NAME.h"

# iOS hardware
mkdir -p "$XCFRAMEWORK_ROOT/ios-arm64"
cp -r "$COMMON" "$XCFRAMEWORK_ROOT/ios-arm64/$FRAMEWORK_NAME.framework"
cp "$TARGET_DIR/aarch64-apple-ios/$BUILD_PROFILE/$LIB_NAME" "$XCFRAMEWORK_ROOT/ios-arm64/$FRAMEWORK_NAME.framework/$FRAMEWORK_NAME"

# iOS simulator, with both platforms as a fat binary for mysterious reasons
mkdir -p "$XCFRAMEWORK_ROOT/ios-arm64_x86_64-simulator"
cp -r "$COMMON" "$XCFRAMEWORK_ROOT/ios-arm64_x86_64-simulator/$FRAMEWORK_NAME.framework"
lipo -create \
  -output "$XCFRAMEWORK_ROOT/ios-arm64_x86_64-simulator/$FRAMEWORK_NAME.framework/$FRAMEWORK_NAME" \
  "$TARGET_DIR/aarch64-apple-ios-sim/$BUILD_PROFILE/$LIB_NAME" \
  "$TARGET_DIR/x86_64-apple-ios/$BUILD_PROFILE/$LIB_NAME"

# Set up the metadata for the XCFramework as a whole.

cp "$WORKING_DIR/Info.plist" "$XCFRAMEWORK_ROOT/Info.plist"

rm -rf "$XCFRAMEWORK_ROOT/common"
