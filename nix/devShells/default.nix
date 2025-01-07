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

  #
  # SGX
  #

  # compile Rust SGX enclaves
  sgx = pkgs.mkShell {
    name = "sgx";
    inputsFrom = [lexePubPkgs.node-release-sgx];
    packages = lib.optionals pkgs.stdenv.isDarwin [
      pkgs.darwin.apple_sdk.frameworks.Security
    ];
  };
}
