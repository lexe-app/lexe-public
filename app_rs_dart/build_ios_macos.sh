#!/usr/bin/env bash

#
# Called by Xcode when building the iOS or macOS native shared library.
# See: `script_phase` in `macos/app_rs_dart.podspec` and `ios/app_rs_dart.podspec`.
#
# You can debug this in relative isolation with:
#
# ```bash
# $ pod lib lint app_rs_dart/macos/app_rs_dart.podspec --verbose
# ```

set -euo pipefail
set -x

# Print out original env/cwd/script path

env | sort

echo "==========="
echo "==========="
echo "==========="

# Important envs from Xcode/CocoaPods:
#
# ACTION (ex: "build", "clean")
# ARCHS (ex: space separated list of "arm64", "armv7", "x86_64")
# CONFIGURATION (ex: "Debug", "Release")
# LD_DYLIB_INSTALL_NAME (ex: "@rpath/app_rs_dart.framework/Versions/A/app_rs_dart")
# PLATFORM_NAME (ex: "macosx", "iphoneos", "iphonesimulator")
# PODS_CONFIGURATION_BUILD_DIR (the build output directory)
# PODS_TARGET_SRCROOT (ex: "app_rs_dart/macos" or "app_rs_dart/ios")
# PRODUCT_NAME (ex: "app_rs_dart")
# SDK_NAMES (ex: "macosx14.4")
# SRCROOT (???)

printenv ACTION
printenv ARCHS
printenv CONFIGURATION
printenv LD_DYLIB_INSTALL_NAME
printenv PLATFORM_NAME
printenv PODS_CONFIGURATION_BUILD_DIR
printenv PODS_TARGET_SRCROOT
printenv PRODUCT_NAME
printenv SDK_NAMES
printenv SRCROOT
printenv TARGET_TEMP_DIR

echo "==========="
echo "==========="
echo "==========="

#
# Reading input from Xcode/CocoaPods envs
#

# Set some useful defaults so we can also run this script free-standing.
ACTION="${ACTION:-build}"
ARCHS="${ARCHS:-arm64}"
CONFIGURATION="${CONFIGURATION:-Debug}"
PLATFORM_NAME="${PLATFORM_NAME:-macosx}"
PRODUCT_NAME="${PRODUCT_NAME:-app_rs_dart}"
LD_DYLIB_INSTALL_NAME="${LD_DYLIB_INSTALL_NAME:-@rpath/$PRODUCT_NAME.framework/Versions/A/$PRODUCT_NAME}"

export NO_COLOR=1

# Place Rust toolchain first.
export PATH="$HOME/.cargo/bin:$PATH";

# app_rs_dart/ directory
APP_RS__APP_RS_DART_DIR="$(dirname "$0")"
# workspace directory
APP_RS__WORKSPACE_DIR="$APP_RS__APP_RS_DART_DIR/.."
APP_RS__TARGET_DIR="$APP_RS__WORKSPACE_DIR/target"

# Read the first arg, so we know which *.podspec is building us (or default to
# macos)
APP_RS__POD_TARGET=""
case "$1" in
  # Default
  "macos" | "") APP_RS__POD_TARGET="macos" ;;
  "ios") APP_RS__POD_TARGET="ios" ;;
  *)
    echo >&2 "error: got unknown target argument from podspec: '$1'"
    exit 1
    ;;
esac

# The final lipo'd output shared libs
APP_RS__OUT=""
if [[ -n "$PODS_CONFIGURATION_BUILD_DIR" ]]; then
  APP_RS__OUT="$PODS_CONFIGURATION_BUILD_DIR/$PRODUCT_NAME/$PRODUCT_NAME.framework/$PRODUCT_NAME"
else
  APP_RS__OUT="$(mktemp)"
  trap 'rm -rf $APP_RS__OUT' EXIT
fi

# Don't use ios/watchos linker for build scripts and proc macros
if [[ "$APP_RS__POD_TARGET" == "ios" ]]; then
  export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/ld
  export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/usr/bin/ld
fi

# Xcode PLATFORM_NAME -> rust target_os 
APP_RS__TARGET_OS=""
case "$PLATFORM_NAME" in
  "macosx") APP_RS__TARGET_OS=darwin ;;
  "iphoneos") APP_RS__TARGET_OS=ios ;;
  "iphonesimulator") APP_RS__TARGET_OS=ios-sim ;;
  *)
    echo >&2 "error: unrecognized PLATFORM_NAME: '$PLATFORM_NAME'"
    exit 1
    ;;
esac

# Xcode CONFIGURATION -> cargo profile
APP_RS__CARGO_PROFILE=""
APP_RS__CARGO_PROFILE_ARG=""
case "$CONFIGURATION" in
  "Release")
    APP_RS__CARGO_PROFILE="release"
    APP_RS__CARGO_PROFILE_ARG="--release"
    ;;
  "Debug")
    APP_RS__CARGO_PROFILE="debug"
    APP_RS__CARGO_PROFILE_ARG=""
    ;;
  *)
    echo >&2 "error: unrecognized Xcode CONFIGURATION: '$CONFIGURATION'"
    exit 1
    ;;
esac

# Xcode ARCHS -> rust target triples
APP_RS__TARGET_TRIPLES=""
# All built libapp_rs.dylib files in target/ directory
APP_RS__TARGET_DIR_LIBS=""
for arch in $ARCHS; do
  if [[ "$arch" == "arm64" ]]; then arch=aarch64; fi
  if [[ "$arch" == "i386" && "$APP_RS__TARGET_OS" != "ios" ]]; then arch=i686; fi
  target="${arch}-apple-$APP_RS__TARGET_OS"
  APP_RS__TARGET_TRIPLES+=" $target"
  APP_RS__TARGET_DIR_LIBS+=" $APP_RS__TARGET_DIR/$target/$APP_RS__CARGO_PROFILE/libapp_rs.dylib"
done

#
# Install any missing rustup target toolchains
#

if ! command -v rustup &> /dev/null; then
  echo >&2 "error: need to install rustup. See README.md"
  exit 1
fi

for target in $APP_RS__TARGET_TRIPLES; do
  if ! rustup target list --installed | grep -Eq "^$target$"; then
    echo >&2 "warning: this build requires rustup toolchain for $target, but it isn't installed (will try rustup next)"
    if ! rustup target add "$target"; then
      echo >&2 "error: failed to install missing rust toolchain with 'rustup target add $target'"
      exit 1
    fi
  fi
done

#
# Build app-rs in the cargo workspace
#

pushd $APP_RS__WORKSPACE_DIR

# Xcode clean -> cargo clean
if [[ "$ACTION" == "clean" ]]; then
  APP_RS__CARGO_TARGET_ARGS=""
  for target in $APP_RS__TARGET_TRIPLES; do
    APP_RS__CARGO_TARGET_ARGS+=" --target=$target"
  done

  cargo clean -p app-rs $APP_RS__CARGO_TARGET_ARGS
  exit 0
fi

# Xcode build -> 'cargo build' for each target
for target in $APP_RS__TARGET_TRIPLES; do
  cargo rustc -p app-rs \
    --lib --crate-type=cdylib \
    --target=$target \
    $APP_RS__CARGO_PROFILE_ARG
done

popd

#
# Use lipo to merge all the separate per-target .dylib's into one universal
# dylib, dumping it into the final output location.
#

lipo -create -output "$APP_RS__OUT" $APP_RS__TARGET_DIR_LIBS

install_name_tool -id "$LD_DYLIB_INSTALL_NAME" "$APP_RS__OUT"

ls -lah $APP_RS__OUT
file $APP_RS__OUT

#
# TODO(phlip9): hook into Xcode's dependency tracking, so we don't have to rerun
# build script every time.
#

# DEP_FILE_DST="$DERIVED_FILE_DIR/${ARCHS}-${EXECUTABLE_NAME}.d"
# echo "" > "$DEP_FILE_DST"
# for target in $APP_RS__TARGET_TRIPLES; do
#   BUILT_SRC="$APP_RS__TARGET_DIR/$target/$APP_RS__CARGO_PROFILE/???"
# 
#  # cargo generates a dep file, but for its own path, so append our rename to it
#  DEP_FILE_SRC="$APP_RS__TARGET_DIR/$target/$APP_RS__CARGO_PROFILE/???"
#  if [ -f "$DEP_FILE_SRC" ]; then
#    cat "$DEP_FILE_SRC" >> "$DEP_FILE_DST"
#  fi
#  echo >> "$DEP_FILE_DST" "${SCRIPT_OUTPUT_FILE_0/ /\\\\ /}: ${BUILT_SRC/ /\\\\ /}"
# done
# cat "$DEP_FILE_DST"
