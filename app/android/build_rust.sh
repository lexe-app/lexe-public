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

# TODO(phlip9): get gradle to tell us which architecture we're building for.
# TODO(phlip9): add `--target=armeabi-v7a` and `--target=x86_64` when
#               publishing. otherwise we get 3x the compile time...

set -o errexit
set -o pipefail
set -o nounset

CARGO_NDK_VERSION="2.12.4"
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

if [[ ! -x "$(command -v cargo)" ]]; then
    echo "Couldn't find 'cargo' binary. Please set up local Rust toolchain."
    exit 1
fi

if [[ ! -x "$(command -v cargo-ndk)" ]]; then
    echo "No cargo-ndk: installing..."
    cargo install --version="$CARGO_NDK_VERSION" cargo-ndk
fi

if [[ ! $(cargo ndk --version) == "cargo-ndk $CARGO_NDK_VERSION" ]]; then
    echo "Different cargo-ndk version: replacing..."
    cargo install --force --version="$CARGO_NDK_VERSION" cargo-ndk
fi

cargo ndk \
    --target=arm64-v8a \
    --output-dir="$SCRIPT_DIR/app/src/main/jniLibs" \
    -- build -p app-rs "$@"
