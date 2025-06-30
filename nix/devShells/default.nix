# Lexe public monorepo dev shells
{
  lib,
  pkgs,
  lexePubPkgs,
}: {
  #
  # app
  #

  # app flutter_rust_bridge codegen
  app-rs-codegen = pkgs.mkShell {
    name = "app-rs-codegen";
    # TODO(phlip9): also `llvm` for dart `ffigen`
    packages = [
      lexePubPkgs.flutter
      lexePubPkgs.rustLexeToolchain
      pkgs.cargo-expand
      # o.w. app-rs-codegen --check says "error: tool 'git' not found" on macOS
      pkgs.git
    ];
  };

  # Android app development toolchains
  app-android = pkgs.mkShellNoCC {
    name = "app-android";

    packages = [
      # flutter/dart
      lexePubPkgs.flutter
      # rust toolchains for Android
      lexePubPkgs.rustLexeToolchainAndroid
      # `cargo ndk` - easily build rust with Android NDK toolchains
      pkgs.cargo-ndk
      # fastlane - app deploy tooling
      pkgs.fastlane
      # bundletool - build and inspect Android app bundles (*.aab)
      pkgs.bundletool
    ];

    env = {
      # flutter SDK directory
      FLUTTER_ROOT = lexePubPkgs.flutter;

      # Android envs
      ANDROID_SDK_ROOT = lexePubPkgs.ANDROID_SDK_ROOT;
      ANDROID_HOME = lexePubPkgs.ANDROID_HOME;
      ANDROID_NDK_ROOT = lexePubPkgs.ANDROID_NDK_ROOT;

      # Java tooling. We can avoid polluting our $PATH by just setting
      # $JAVA_HOME, which Android tooling looks for.
      JAVA_HOME = lexePubPkgs.JAVA_HOME;
    };

    shellHook = ''
      export GRADLE_USER_HOME="$HOME/.gradle";
    '';

    meta = {
      # Missing: aarch64-linux
      # Reason: Android SDK doesn't provide pre-built binaries for it.
      platforms = ["x86_64-linux" "aarch64-darwin"];
    };
  };

  # iOS/macOS app development toolchains
  #
  # Unfortunately, the nix `stdenv` clobbers the system Xcode tools, which we
  # need to use for production iOS/macOS releases. This hacky `mkMinShell` lets
  # us use `nix develop` while exactly controlling which envs we modify.
  app-ios-macos = lexePubPkgs.mkMinShell {
    name = "app-ios-macos";

    packages = [
      # flutter/dart
      lexePubPkgs.flutter
      # rustc/cargo - rust toolchains for iOS/macOS
      lexePubPkgs.rustLexeToolchainiOSmacOS
      # fastlane - app deploy tooling
      pkgs.fastlane
      # pod
      pkgs.cocoapods
      # idevicesyslog (among others) - view logs from attached iOS device
      pkgs.libimobiledevice
      # Use standard rsync. macOS rsync (OpenBSD) doesn't copy Flutter.framework
      # with the right permissions.
      pkgs.rsync
    ];

    env = {
      FLUTTER_ROOT = lexePubPkgs.flutter;
      # fastlane claims build uploads will break without this
      # <https://docs.fastlane.tools/getting-started/ios/setup/>
      LANG = "en_US.UTF-8";
      LC_ALL = "en_US.UTF-8";
      LEXE_XCODE_VERSION = "16.3";
      LEXE_MACOS_SDK_VERSION = "15.4";
      LEXE_IOS_SDK_VERSION = "18.4";
    };

    shellHook = ''
      unset GEM_HOME

      if [[ ! -d /Applications/Xcode.app ]]; then
        echo >&2 "error: Xcode is not installed (/Applications/Xcode.app is missing)"
        echo >&2 "suggestion: install Xcode from the App Store"
        exit 1
      fi

      # Check Xcode.app version
      actualXcodeVers="$(plutil -extract CFBundleShortVersionString \
        raw -n -o - /Applications/Xcode.app/Contents/version.plist)"
      if [[ "$actualXcodeVers" != "$LEXE_XCODE_VERSION" ]]; then
        echo >&2 "error: you're using a different Xcode version than we expect."
        echo >&2 ""
        echo >&2 "      from: /Applications/Xcode.app/Contents/version.plist"
        echo >&2 "    actual: $actualXcodeVers"
        echo >&2 "  expected: $LEXE_XCODE_VERSION"
        echo >&2 ""
        echo >&2 "suggestion: update Xcode in the App Store"
        exit 1
      fi

      # Check it's system xcodebuild
      if [[ "$(command -v xcodebuild)" != "/usr/bin/xcodebuild" ]]; then
        echo >&2 "error: xcodebuild should be using the system path"
        echo >&2 ""
        echo >&2 "    actual: $(command -v xcodebuild || echo ' ')"
        echo >&2 "  expected: /usr/bin/xcodebuild"
        echo >&2 ""
        echo >&2 "suggestion: install Xcode CommandLineTools via 'xcode-select -install'"
        echo >&2 "or check your PATH"
        exit 1
      fi

      # Check xcodebuild version
      actualXcodebuildVers="$(xcodebuild -version | head -n 1 | cut -d ' ' -f 2)"
      if [[ "$actualXcodebuildVers" != "$LEXE_XCODE_VERSION" ]]; then
        echo >&2 "error: you're using a different xcodebuild version than we expect."
        echo >&2 ""
        echo >&2 "      from: xcodebuild -version"
        echo >&2 "    actual: $actualXcodebuildVers"
        echo >&2 "  expected: $LEXE_XCODE_VERSION"
        echo >&2 ""
        echo >&2 "suggestion: update Xcode in the App Store"
        exit 1
      fi

      # Check macOS SDK version
      if ! xcodebuild -version -sdk "macosx$LEXE_MACOS_SDK_VERSION" >/dev/null; then
        echo >&2 "error: couldn't find macOS SDK version $LEXE_MACOS_SDK_VERSION"
        echo >&2 "suggestion: open Xcode and poke around? IDK"
        exit 1
      fi

      # Check iOS SDK version
      if ! xcodebuild -version -sdk "iphoneos$LEXE_IOS_SDK_VERSION" >/dev/null; then
        echo >&2 "error: couldn't find iOS SDK version $LEXE_IOS_SDK_VERSION"
        echo >&2 "suggestion: open Xcode and poke around? IDK"
        exit 1
      fi

      # Check it's system xcrun
      if [[ "$(command -v xcrun)" != "/usr/bin/xcrun" ]]; then
        echo >&2 "error: xcrun should be using the system path"
        echo >&2 ""
        echo >&2 "    actual: $(command -v xcrun || echo ' ')"
        echo >&2 "  expected: /usr/bin/xcrun"
        echo >&2 ""
        echo >&2 "suggestion: install Xcode CommandLineTools via 'xcode-select -install'"
        echo >&2 "or check your PATH"
        exit 1
      fi
    '';
  };

  #
  # SGX
  #

  # compile Rust SGX enclaves
  sgx = pkgs.mkShell {
    name = "sgx";
    inputsFrom = [lexePubPkgs.node-release-sgx];
  };
}
