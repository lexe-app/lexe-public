#!/usr/bin/env bash

# This script builds the `app-rs` crate as a shared library for the different
# android targets.
#
# It uses [`cargo ndk`](https://github.com/bbqsrc/cargo-ndk) to do the heavy
# lifting and installs it if not already installed.
#
# Any additional arguments passed to this script are also passed through to
# the underlying `cargo build` invocation.
#
# See: `app/android/app/build.gradle` for how this script is used when hooked
#      into gradle build.

# TODO(phlip9): get gradle to tell us the target android platform API level.
# TODO(phlip9): get gradle to tell us which architecture we're building for.
# TODO(phlip9): add `--target=armeabi-v7a` and `--target=x86_64` when
#               publishing. otherwise we get 3x the compile time...

set -o errexit
set -o pipefail
set -o nounset

CARGO_NDK_VERSION="3.0.1"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
TARGET="aarch64-linux-android"

if [[ ! -x "$(command -v cargo)" ]]; then
    echo "error: Couldn't find 'cargo' or 'rustup' binary. \
Please set up local Rust toolchain."
    exit 1
fi

if [[ ! -x "$(command -v cargo-ndk)" ]]; then
    echo "info: Installing cargo-ndk"
    cargo install --version="$CARGO_NDK_VERSION" cargo-ndk
elif [[ ! $(cargo ndk --version) == "cargo-ndk $CARGO_NDK_VERSION" ]]; then
    echo "info: Updating cargo-ndk to version $CARGO_NDK_VERSION"
    cargo install --force --version="$CARGO_NDK_VERSION" cargo-ndk
fi

if ! rustup target list --installed | grep -q "$TARGET"; then
    echo "info: Installing missing Rust toolchain for target: $TARGET"
    rustup target add "$TARGET"
fi

cargo ndk \
    --target=arm64-v8a \
    --output-dir="$SCRIPT_DIR/app/src/main/jniLibs" \
    -- build -p app-rs "$@"
