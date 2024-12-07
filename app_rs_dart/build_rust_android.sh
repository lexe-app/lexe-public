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
# TODO(phlip9): add `armv7-linux-androideabi` target when publishing. otherwise
#               we get 2x the compile time during development...

set -e
set -o pipefail
# set -x

export NO_COLOR=1

CARGO_NDK_VERSION="3.5.7"

# Important envs passed to us from `app_rs_dart/android/build.gradle`:
#
# ANDROID_NDK_HOME (ex: "/Users/phlip9/.local/android/ndk/23.1.7779620")
# APP_RS__OUT_DIR (ex: "/Users/phlip9/dev/lexe/public/app/build/app_rs_dart/jniLibs/release")
# APP_RS__COMPILE_SDK_VERSION (ex: "34")
# APP_RS__TARGETS (ex: "aarch64-linux-android armv7-linux-androideabi")

#
# Read input from gradle
#

APP_RS__COMPILE_SDK_VERSION="${APP_RS__COMPILE_SDK_VERSION:-34}"
APP_RS__TARGETS="${APP_RS__TARGETS:-"aarch64-linux-android"}"

# If we run this script standalone, just dump the output in a tempdir.
if [[ -z $APP_RS__OUT_DIR ]]; then
  APP_RS__OUT_DIR="$(mktemp -d)"
  trap 'rm -rf $APP_RS__OUT_DIR' EXIT
fi

#
# Ensure we always build from workspace directory
#

# app_rs_dart/ directory
APP_RS__APP_RS_DART_DIR="$(realpath "$(dirname "$0")")"
# workspace directory
APP_RS__WORKSPACE_DIR="$(dirname "$APP_RS__APP_RS_DART_DIR")"

# Enter workspace directory for duration of script
pushd "$APP_RS__WORKSPACE_DIR"

#
# Ensure toolchains are installed
#

# Ensure `cargo` is installed
if ! command -v cargo &> /dev/null; then
  echo >&2 "error: need to install cargo. See README.md"
  echo >&2 "  > suggestion:   nix develop .#app-android"
  exit 1
fi

# Ensure `cargo ndk` is installed
if ! command -v cargo-ndk &> /dev/null; then
  echo >&2 "error: need to install cargo-ndk"
  echo >&2 "  > suggestion:   nix develop .#app-android"
  echo >&2 "            or:   cargo install --version=$CARGO_NDK_VERSION cargo-ndk"
  exit 1
fi

# Ensure `cargo ndk` has the right version
actualCargoNdk="$(cargo ndk --version)"
expectedCargoNdk="cargo-ndk $CARGO_NDK_VERSION"
if [[ $actualCargoNdk != "$expectedCargoNdk" ]]; then
  echo >&2 "error: \"$actualCargoNdk\" != \"$expectedCargoNdk\""
  echo >&2 "  > suggestion:   nix develop .#app-android"
  echo >&2 "            or:   cargo install --force --version=$CARGO_NDK_VERSION cargo-ndk"
  exit 1
fi

# Ensure rust toolchains are installed for targets
for target in $APP_RS__TARGETS; do
  if ! rustc --target "$target" --print target-libdir &> /dev/null; then
    echo >&2 "error: missing Rust toolchain for target $target"
    echo >&2 "  > suggestion:   nix develop .#app-android"
    echo >&2 "            or:   rustup target add $target"
    exit 1
  fi
done

#
# `cargo ndk build` the libapp_rs.so shared library
#

# Try to sanitize paths in the output library. These remaps get applied from
# last-to-first.
RUSTFLAGS="\
  --remap-path-prefix $HOME=/home \
  --remap-path-prefix $HOME/.cargo=/cargo \
  --remap-path-prefix $HOME/.cargo/registry/src/index.crates.io-6f17d22bba15001f=/crates-io \
  --remap-path-prefix $APP_RS__WORKSPACE_DIR=/lexe"

# Envs to propagate to `cargo ndk build`
clean_envs=("PATH=$PATH" "HOME=$HOME" LC_ALL="$LC_ALL" RUSTFLAGS="$RUSTFLAGS")

# Only propagate these envs if they're already set
#
# People need ANDROID_HOME set if they're to develop on android at all;
# `cargo ndk` is also smart enough to figure out _an_ NDK to use if that's all
# it's given. The most accurate ofc is the ANDROID_NDK_HOME we get from gradle,
# but that might not be set when running this script standalone.
# TODO(phlip9): do we still need `ANDROID_NDK_HOME`?
conditional_envs=("ANDROID_SDK_ROOT" "ANDROID_NDK_ROOT" "ANDROID_HOME" "ANDROID_NDK_HOME")
for env in "${conditional_envs[@]}"; do
  if printenv "$env"; then
    clean_envs+=("$env=$(printenv "$env")")
  fi
done

# --target=$target
targetArgs=()
for target in $APP_RS__TARGETS; do
  targetArgs+=("--target=$target")
done

set -x

# Run `cargo ndk build` in a clean env
# Short args (-i) ensure this works with non-coreutils /usr/bin/env on macOS.
env -i "${clean_envs[@]}" \
  cargo ndk \
  "${targetArgs[@]}" \
  --output-dir="$APP_RS__OUT_DIR" \
  --platform="$APP_RS__COMPILE_SDK_VERSION" \
  -- rustc --lib --crate-type=cdylib -p app-rs "$@"

set +x

# Restore cwd
popd
