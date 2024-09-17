# Blockstream/electrs - indexed BTC chain REST API server
#
# Used in integration tests (see `common/src/regtest.rs`)
# TODO(phlip9): broken until this PR lands <https://github.com/Blockstream/electrs/pull/109>.
{
  buildRustSccache,
  darwin ? {},
  fetchFromGitHub,
  iconv,
  lib,
  rocksdb,
  rustPlatform,
  stdenv,
  vendorCargoDeps,
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
    cargoToml = src + "/Cargo.toml";
    src = src;
    cargoVendorDir = vendorCargoDeps {
      cargoLock = src + "/Cargo.lock";
      gitDepOutputHashes = {
        "git+https://github.com/shesek/rust-jsonrpc?branch=202201-nonarray#aaa0af349bd4885a59f6f6ba1753e78279014f98" = "sha256-lSNkkQttb8LnJej4Vfe7MrjiNPOuJ5A6w5iLstl9O1k=";
        "git+https://github.com/shesek/electrumd?rev=6eac0b7b1f2447472016e2c1473a6284f7f8648e#6eac0b7b1f2447472016e2c1473a6284f7f8648e" = "sha256-s1/laailcwOmqjAPJnuqe7Y45Bvxwqw8EjKN54BS5gI=";
        "git+https://github.com/Blockstream/rust-electrum-client?rev=d3792352992a539afffbe11501d1aff9fd5b919d#d3792352992a539afffbe11501d1aff9fd5b919d" = "sha256-HDRdGS7CwWsPXkA1HdurwrVu4lhEx0Ay8vHi08urjZ0=";
      };
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

    buildInputs = lib.optionals stdenv.isDarwin [
      darwin.apple_sdk.frameworks.Security
      # Not sure why this is required?
      iconv
    ];

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
