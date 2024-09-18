# Blockstream/electrs - indexed BTC chain REST API server
#
# Used in integration tests (see `common/src/regtest.rs`)
{
  buildRustSccache,
  darwin ? {},
  fetchFromGitHub,
  lib,
  rocksdb,
  rustPlatform,
  stdenv,
  vendorCargoDeps,
}: let
  # commit 2024-09-16
  rev = "891426ab458eaf807442d8bfdb7b1b7386d358ea";
  shortRev = builtins.substring 0 8 rev;

  src = fetchFromGitHub {
    owner = "Blockstream";
    repo = "electrs";
    rev = rev;
    hash = "sha256-5IBoKLfKKNc/Ju17AqQf0ALn9AvNfTfDvth5R15aq8U=";
  };
in
  buildRustSccache {
    cargoToml = src + "/Cargo.toml";
    src = src;
    cargoVendorDir = vendorCargoDeps {
      cargoLock = src + "/Cargo.lock";
      gitDepOutputHashes = {
        "git+https://github.com/Blockstream/rust-electrum-client?rev=d3792352992a539afffbe11501d1aff9fd5b919d#d3792352992a539afffbe11501d1aff9fd5b919d" = "sha256-HDRdGS7CwWsPXkA1HdurwrVu4lhEx0Ay8vHi08urjZ0=";
        "git+https://github.com/shesek/electrumd?rev=b35d9db285d932cb3c2296beab65e571a2506349#b35d9db285d932cb3c2296beab65e571a2506349" = "sha256-QsoMD2uVDEITuYmYItfP6BJCq7ApoRztOCs7kdeRL9Y=";
        "git+https://github.com/shesek/rust-jsonrpc?branch=202201-nonarray#aaa0af349bd4885a59f6f6ba1753e78279014f98" = "sha256-lSNkkQttb8LnJej4Vfe7MrjiNPOuJ5A6w5iLstl9O1k=";
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
