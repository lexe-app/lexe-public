# Blockstream/electrs - indexed BTC chain REST API server
#
# Used in integration tests (see `common/src/regtest.rs`)
{
  buildRustSccache,
  craneLib,
  darwin ? {},
  fetchFromGitHub,
  lib,
  rocksdb,
  rustPlatform,
  stdenv,
}: let
  # commit 2024-02-27
  rev = "49a71805a2c15852a4fa0450bcb5a4a4a36c89d8";
  shortRev = builtins.substring 0 8 rev;

  src = fetchFromGitHub {
    owner = "Blockstream";
    repo = "electrs";
    rev = rev;
    hash = "sha256-bPEkdNGlQhpE4cj8K1yQWINqZ2MIgyrrYnp/IA9lu4Y=";
  };
in
  buildRustSccache {
    cargoToml = "${src}/Cargo.toml";
    src = src;
    cargoVendorDir = craneLib.vendorCargoDeps {
      cargoLock = "${src}/Cargo.lock";
    };
    skipDepsOnlyBuild = true;

    pname = "electrs";
    version = shortRev;
    doCheck = false;

    cargoExtraArgs = builtins.concatStringsSep " " [
      "--offline"
      "--locked"
      "--package=electrs"
      "--bin=electrs"
      "--no-default-features"
    ];

    nativeBuildInputs = [
      # needed for librocksdb-sys
      rustPlatform.bindgenHook
    ];

    buildInputs = lib.optionals stdenv.isDarwin [darwin.apple_sdk.frameworks.Security];

    # link rocksdb dynamically
    ROCKSDB_INCLUDE_DIR = "${rocksdb}/include";
    ROCKSDB_LIB_DIR = "${rocksdb}/lib";

    # make sure it at least runs
    doInstallCheck = true;
    installCheckPhase = ''
      $out/bin/electrs --version
    '';

    meta = with lib; {
      description = "An efficient re-implementation of Electrum Server in Rust";
      homepage = "https://github.com/Blockstream/electrs";
      license = licenses.mit;
      mainProgram = "electrs";
    };
  }
