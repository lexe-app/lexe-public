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
  app-rs-codegen = pkgs.mkShellNoCC {
    name = "app-rs-codegen";
    # TODO(phlip9): also `llvm` for dart `ffigen`
    packages = [pkgs.cargo-expand];
  };

  # Android app development toolchains
  app-android = pkgs.mkShellNoCC {
    name = "app-android";

    packages = [
      lexePubPkgs.flutter
      lexePubPkgs.rustLexeToolchainAndroid
      pkgs.cargo-ndk
    ];

    env = {
      FLUTTER_SDK = lexePubPkgs.flutter;

      ANDROID_SDK_ROOT = lexePubPkgs.ANDROID_SDK_ROOT;
      ANDROID_NDK_ROOT = lexePubPkgs.ANDROID_NDK_ROOT;

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
