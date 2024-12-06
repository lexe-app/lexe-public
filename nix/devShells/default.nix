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
