# justfile for the Lexe app (Flutter + flutter_rust_bridge to app-rs).
#
# Recipes assume cwd = public/app/. This module lives in public/just/app/, so
# `set working-directory` points cwd back at the app dir; `just` applies it on
# every `just app <recipe>` invocation (from root or public/). Inline shebang
# recipes inherit the same cwd.

set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '../../app'

# select a recipe interactively
default:
    @just --choose

# --- dart-define plumbing --- #
#
# Environment variables don't work for flutter (mobile!) apps, so we need to set
# things like logging config at build time, via --dart-define. Retrieve these
# defines with `String.fromEnvironment` in dart.
#
# This is a disgusting sh-ism that basically prints `--dart-define=XXX=$XXX`
# only when $XXX is already set. In other words, we propagate specific envs to
# --dart-define flags.
#
# For future reference, we need "${XXX:-}" (i.e., value $XXX or "" if unset) o/w
# we get `sh: XXX: unbound variable` errors.
#
# [ ! -z ... ] means "if not zero-length string"...

dart-defines := '$( \
[ ! -z "${RUST_LOG:-}" ] && echo "--dart-define=RUST_LOG=$RUST_LOG"; \
[ ! -z "${DEPLOY_ENVIRONMENT:-}" ] && echo "--dart-define=DEPLOY_ENVIRONMENT=$DEPLOY_ENVIRONMENT"; \
[ ! -z "${NETWORK:-}" ] && echo "--dart-define=NETWORK=$NETWORK"; \
[ ! -z "${SGX:-}" ] && echo "--dart-define=SGX=$SGX"; \
[ ! -z "${DEV_GATEWAY_URL:-}" ] && echo "--dart-define=DEV_GATEWAY_URL=$DEV_GATEWAY_URL"; \
)'

# Ex:
#
# ```bash
# $ RUST_LOG=info SGX=true just app test-dart-defines
# --dart-define=RUST_LOG=info --dart-define=SGX=true
# ```

test-dart-defines:
    echo {{ dart-defines }}

# --- build / run --- #

flutter *args:
    flutter {{ args }}

pub-get:
    flutter pub get
    cd ../app_rs_dart && flutter pub get
    cd ../app_embed_git_rev && dart pub get
    cd ../app_lints && dart pub get

build-runner-build *args:
    dart run build_runner build --delete-conflicting-outputs {{ args }}

# run the app Rust+Dart ffi codegen
rs-codegen *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v flutter &> /dev/null; then
      echo >&2 "error: flutter_rust_bridge codegen requires 'flutter'"
      echo >&2 ""
      echo >&2 "    nix develop .#app-rs-codegen"
      echo >&2 ""
      exit 1
    fi
    if ! command -v cargo-expand &> /dev/null; then
      echo >&2 "error: flutter_rust_bridge codegen requires 'cargo-expand'"
      echo >&2 ""
      echo >&2 "    nix develop .#app-rs-codegen"
      echo >&2 ""
      exit 1
    fi
    pushd ../app_rs_dart && flutter pub get && popd
    cargo run -p app-rs-codegen -- {{ args }}

rs-freezed *args:
    cd ../app_rs_dart && dart run build_runner build {{ args }}

# check that the Rust+Dart ffi codegen is up-to-date
rs-codegen-check:
    just app rs-codegen --check

# lint native app-rs crate, built for android target
rs-lint-android *args:
    cd .. && cargo ndk \
        --target=aarch64-linux-android \
        --platform=34 \
        -- clippy -p app-rs {{ args }}

# lint native app-rs crate, built for ios target
rs-lint-ios *args:
    cd .. && cargo clippy \
        --target=aarch64-apple-ios \
        -p app-rs {{ args }}

run *args:
    flutter run {{ dart-defines }} {{ args }}

build kind *args:
    flutter build {{ kind }} {{ dart-defines }} {{ args }}

# run the app against local dev. config is pulled from <repo-root>/.env
run-dev *args:
    dotenvy -f "$(git rev-parse --show-toplevel)/.env" \
      just app run --flavor=dev {{ args }}

# run the app against staging/testnet
run-staging *args:
    DEPLOY_ENVIRONMENT=staging NETWORK=testnet3 SGX=true \
      just app run --flavor=staging {{ args }}

# run the app against production/mainnet
run-prod *args:
    DEPLOY_ENVIRONMENT=prod NETWORK=mainnet SGX=true \
      just app run --flavor=prod {{ args }}

# run the app in "design" mode for quickly iterating on pages and components.
run-design *args:
    DEPLOY_ENVIRONMENT=dev NETWORK=regtest SGX=false \
      just app run --flavor=design --target=lib/design_mode/main.dart {{ args }}

# --- test / lint / fmt --- #

# run app unit tests on local machine
test *args:
    cd .. && cargo rustc --lib --crate-type=cdylib -p app-rs
    flutter test {{ dart-defines }} {{ args }}
    cd ../app_lints && dart test {{ args }}

# run on-device app integration tests
integration *args:
    flutter test integration_test {{ dart-defines }} {{ args }}

# lint the app's dart code
lint *args:
    flutter analyze {{ args }}
    cd ../app_rs_dart && flutter analyze {{ args }}
    cd ../app_embed_git_rev && flutter analyze {{ args }}
    cd ../app_lints && flutter analyze {{ args }}

    @# TODO: Figure out why this takes like 27sec to run via CLI
    @# dart run custom_lint

# fix some app lints that have quickfixes available
fix *args:
    dart fix --apply {{ args }}
    cd ../app_rs_dart && dart fix --apply {{ args }}
    cd ../app_embed_git_rev && dart fix --apply {{ args }}
    cd ../app_lints && dart fix --apply {{ args }}

    dart run custom_lint --fix {{ args }}

    just app fmt

fmt:
    dart format --line-length=80 .
    cd ../app_rs_dart && dart format --line-length=80 .
    cd ../app_embed_git_rev && dart format --line-length=80 .
    cd ../app_lints && dart format --line-length=80 .

fmt-check:
    dart format --set-exit-if-changed --line-length=80 .
    cd ../app_rs_dart && dart format --set-exit-if-changed --line-length=80 .
    cd ../app_embed_git_rev && dart format --set-exit-if-changed --line-length=80 .
    cd ../app_lints && dart format --set-exit-if-changed --line-length=80 .

# A subset of local app CI suitable for AIs to quickly check their work.
# Root justfile re-exposes this as `just ai-app-ci`.

ci:
    just app lint --no-pub --no-preamble --no-congratulate
    just app fmt-check
    @# Annoyingly causes diffs to temporarily appear
    @# just app rs-codegen-check

local-ci:
    just app rs-codegen-check
    just app fmt-check
    just app lint
    just app test
    just app integration

# --- on-device log tailing --- #

# Tail Lexe logs from an attached Android device. Useful for debugging against prod.
logs-android args="lexeapp:V flutter:V *:S":
    $ANDROID_SDK_ROOT/platform-tools/adb logcat {{ args }}

# Tail Lexe logs from an attached iOS device. Useful for debugging against prod.
logs-ios *args:
    idevicesyslog --process "Lexe|Lexe Design|Lexe Dev|Lexe Staging" \
        --match "flutter:" \
        {{ args }}

# --- fastlane passthroughs --- #
#
# These don't need lexe secrets themselves; the secrets get injected by the
# root-only `app-android-with-api-key` / `app-ios-with-api-key` wrappers.

# run fastlane in the app android dir
fastlane-android *args:
    cd android && fastlane {{ args }}

# run fastlane in the app iOS dir
fastlane-ios *args:
    cd ios && fastlane {{ args }}

# --- android build & inspect --- #

# build the production Android app release artifact
build-prod-android *args:
    DEPLOY_ENVIRONMENT=prod NETWORK=mainnet SGX=true \
      just app build appbundle --flavor=prod --release {{ args }}

# print the Android app's requested permissions from the compiled bundle.
android-bundle-permissions:
    bundletool dump manifest \
        --bundle=build/app/outputs/bundle/prodRelease/app-prod-release.aab \
        --xpath=/manifest/uses-permission/@android:name

# read the combined version from the built .aab bundle (ex: "1.2.3+45")
# [no-cd] so the caller's cwd is preserved; the release script
# (/just/release-app-android.sh) passes a repo-root-relative path.

[no-cd]
android-bundle-version path:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ ! -f "{{ path }}" ]] && exit 0

    version_name="$(bundletool dump manifest \
        --bundle "{{ path }}" \
        --xpath "/manifest/@android:versionName")"
    version_code="$(bundletool dump manifest \
        --bundle "{{ path }}" \
        --xpath "/manifest/@android:versionCode")"
    printf '%s+%s\n' "$version_name" "$version_code"

# read the embedded git revision from the built .aab bundle
[no-cd]
android-bundle-git-rev path:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ ! -f "{{ path }}" ]] && exit 0

    unzip -p "{{ path }}" base/assets/flutter_assets/assets/git_rev

# build android app and check 16KiB page-size compatibility
android-test-16kb-page-sizes *args:
    just app build-prod-android {{ args }}
    just app android-lint-16kb-page-sizes build/app/outputs/bundle/prodRelease/app-prod-release.aab

android-lint-16kb-page-sizes path:
    ../just/app/android-lint-16kb-page-sizes.sh "{{ path }}"

# --- ios build & inspect --- #

# build the production iOS app release artifact
build-prod-ios *args:
    DEPLOY_ENVIRONMENT=prod NETWORK=mainnet SGX=true \
        just app build ipa --flavor=prod --release {{ args }}

# read the combined version from the built .ipa bundle (ex: "1.2.3+45")
# [no-cd] so the caller's cwd is preserved; the release script
# (/just/release-app-ios.sh) passes a repo-root-relative path.

[no-cd]
ios-bundle-version path:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ ! -f "{{ path }}" ]] && exit 0

    version_name="$(unzip -p "{{ path }}" Payload/Lexe.app/Info.plist \
        | plutil -extract CFBundleShortVersionString raw -)"
    version_code="$(unzip -p "{{ path }}" Payload/Lexe.app/Info.plist \
        | plutil -extract CFBundleVersion raw -)"
    printf '%s+%s\n' "$version_name" "$version_code"

# read the embedded git revision from the built .ipa bundle
[no-cd]
ios-bundle-git-rev path:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ ! -f "{{ path }}" ]] && exit 0

    unzip -p "{{ path }}" Payload/Lexe.app/Frameworks/App.framework/flutter_assets/assets/git_rev

# --- misc --- #

# print the app version from pubspec.yaml (ex: "1.2.3+45")
repo-version:
    rg --no-line-number --only-matching --replace '$1' \
        "^version: ([[[:digit:]]\.\+]+)$" \
        pubspec.yaml
