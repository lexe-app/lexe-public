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

set -e
set -o pipefail
set -x

# Important envs passed to us from gradle:
#
# ANDROID_NDK_HOME (ex: "/Users/phlip9/.local/android/ndk/23.1.7779620")
# APP_RS__OUT_DIR (ex: "/Users/phlip9/dev/lexe/public/app/build/app_rs_dart/jniLibs/release")
# APP_RS__COMPILE_SDK_VERSION (ex: "34")

CARGO_NDK_VERSION="3.5.4"
TARGET="aarch64-linux-android"
APP_RS__COMPILE_SDK_VERSION="${APP_RS__COMPILE_SDK_VERSION:-34}"

# If we run this script standalone, just dump the output in a tempdir.
if [[ -z $APP_RS__OUT_DIR ]]; then
  APP_RS__OUT_DIR="$(mktemp -d)"
  trap 'rm -rf $APP_RS__OUT_DIR' EXIT
fi

# Ensure cargo is installed
if ! command -v cargo &> /dev/null; then
  echo >&2 "error: need to install cargo. See README.md"
  exit 1
fi

# Ensure rustup is installed
if ! command -v rustup &> /dev/null; then
  echo >&2 "error: need to install rustup. See README.md"
  exit 1
fi

# Ensure `cargo ndk` is installed with the desired version
if ! command -v cargo-ndk &> /dev/null; then
  echo "info: Installing cargo-ndk"
  cargo install --version="$CARGO_NDK_VERSION" cargo-ndk
elif [[ $(cargo ndk --version) != "cargo-ndk $CARGO_NDK_VERSION" ]]; then
  echo "info: Updating cargo-ndk to version $CARGO_NDK_VERSION"
  cargo install --force --version="$CARGO_NDK_VERSION" cargo-ndk
fi

# Ensure rustup is installed
if ! command -v rustup &> /dev/null; then
  echo >&2 "error: need to install rustup. See README.md"
  exit 1
fi

# Ensure rust toolchains are installed for targets
if ! rustup target list --installed | grep -Eq "^$TARGET$"; then
  echo "info: Installing missing Rust toolchain for target: $TARGET"
  if ! rustup target add "$TARGET"; then
    echo >&2 "error: failed to install missing rust toolchain with 'rustup target add $TARGET'"
    exit 1
  fi
fi

# Envs to propagate to `cargo ndk build`
clean_envs=("PATH=$PATH" "HOME=$HOME" LC_ALL="$LC_ALL")

# Only propagate these envs if they're already set
#
# People need ANDROID_HOME set if they're to develop on android at all;
# `cargo ndk` is also smart enough to figure out _an_ NDK to use if that's all
# it's given. The most accurate ofc is the ANDROID_NDK_HOME we get from gradle,
# but that might not be set when running this script standalone.
conditional_envs=("ANDROID_HOME" "ANDROID_NDK_HOME")
for env in "${conditional_envs[@]}"; do
  if printenv "$env"; then
    clean_envs+=("$env=$(printenv "$env")")
  fi
done

# TODO(phlip9): get backtraces working...
# --no-strip

# Run `cargo ndk build` in a clean env
env --ignore-environment "${clean_envs[@]}" \
  cargo ndk \
  --target="$TARGET" \
  --output-dir="$APP_RS__OUT_DIR" \
  --platform="$APP_RS__COMPILE_SDK_VERSION" \
  -- rustc --lib --crate-type=cdylib -p app-rs "$@"
