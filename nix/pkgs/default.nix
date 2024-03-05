# lexe public monorepo nix packages set
{
  lib,
  pkgs,
  crane,
  lexePubLib,
}: rec {
  # cargo workspace Cargo.toml & Cargo.lock info
  workspaceRoot = ../..;
  workspaceToml = workspaceRoot + "/Cargo.toml";
  workspaceLock = workspaceRoot + "/Cargo.lock";
  workspaceTomlParsed = builtins.fromTOML (builtins.readFile workspaceToml);
  workspaceVersion = workspaceTomlParsed.workspace.package.version;

  # Instantiate the rust toolchain from our `rust-toolchain.toml`.
  rustLexeToolchain =
    pkgs.rust-bin.fromRustupToolchainFile ../../rust-toolchain.toml;

  # `crane` cargo builder instantiated with our rust toolchain settings.
  craneLib = (crane.mkLib pkgs).overrideToolchain rustLexeToolchain;

  # workspace source directory, cleaned of anything not needed to build rust
  # code
  srcRust = lib.cleanSourceWith {
    src = workspaceRoot;
    filter = path: type:
      (craneLib.filterCargoSources path type) || (lib.hasSuffix ".der" path);
  };

  # Download all cargo deps from the workspace Cargo.lock
  cargoVendorDir = craneLib.vendorCargoDeps {
    cargoLock = workspaceLock;
  };

  # Use the latest clang/llvm for cross-compiling SGX.
  llvmPackages = pkgs.llvmPackages_latest;

  # Shim a small set of libc fns so we can cross-compile SGX without glibc.
  sgx-libc-shim = pkgs.callPackage ./sgx-libc-shim.nix {};

  # Inject env vars for cross-compiling to SGX into your `buildPhase`.
  sgxCrossEnvBuildHook = pkgs.callPackage ./sgxCrossEnvBuildHook.nix {
    inherit llvmPackages sgx-libc-shim;
  };

  # Generic rust builder for non-SGX crates. Supports shared nix cargo build
  # caching with `sccache`. Use this for builds that don't require 100%
  # reproducibility.
  buildRustSccache = pkgs.callPackage ./buildRustSccache.nix {
    inherit craneLib cargoVendorDir lexePubLib srcRust workspaceVersion;
  };

  # bitcoind - Bitcoin core wallet (just an alias)
  bitcoind = pkgs.bitcoind;

  # Blockstream fork of electrs BTC chain index server, used in integration tests
  blockstream-electrs = pkgs.callPackage ./blockstream-electrs.nix {
    inherit buildRustSccache craneLib;
  };

  # rust-sgx repo source
  rustSgxRepo = lexePubLib.parseCargoLockGitDep {
    cargoLockContents = builtins.readFile workspaceLock;
    githubUrl = "https://github.com/lexe-app/rust-sgx";
  };
  rustSgxSrc = builtins.fetchGit (rustSgxRepo // {shallow = true;});
  rustSgxCargoVendorDir = craneLib.vendorCargoDeps {
    cargoLock = "${rustSgxSrc}/Cargo.lock";
  };

  # Converts a compiled `x86_64-fortanix-unknown-sgx` ELF binary into
  # a `.sgxs` enclave file.
  ftxsgx-elf2sgxs = pkgs.callPackage ./ftxsgx-elf2sgxs.nix {
    inherit buildRustSccache rustSgxSrc rustSgxCargoVendorDir;
  };

  # A hook that runs `ftxsgx-elf2sgxs` on the output binary in the
  # `postFixup` phase.
  elf2sgxsFixupHook = pkgs.callPackage ./elf2sgxsFixupHook.nix {
    ftxsgx-elf2sgxs = ftxsgx-elf2sgxs;
  };

  # Run to detect the current system's support for Intel SGX. Only builds and
  # runs on `x86_64-linux`.
  sgx-detect = pkgs.callPackage ./sgx-detect.nix {
    inherit buildRustSccache rustSgxSrc rustSgxCargoVendorDir;
  };

  # Generic builder for Rust SGX crates.
  buildRustSgxPackage = pkgs.callPackage ./buildRustSgxPackage.nix {
    inherit craneLib cargoVendorDir srcRust sgxCrossEnvBuildHook elf2sgxsFixupHook;
  };

  # User's node SGX enclave
  node-release-sgx = buildRustSgxPackage {
    cargoToml = ../../node/Cargo.toml;
    isSgx = true;
    isRelease = true;
  };
  node-release-nosgx = buildRustSgxPackage {
    cargoToml = ../../node/Cargo.toml;
    isSgx = false;
    isRelease = true;
  };
  node-debug-sgx = buildRustSgxPackage {
    cargoToml = ../../node/Cargo.toml;
    isSgx = true;
    isRelease = false;
  };
  node-debug-nosgx = buildRustSgxPackage {
    cargoToml = ../../node/Cargo.toml;
    isSgx = false;
    isRelease = false;
  };

  # Binary for running SGX enclaves.
  run-sgx = buildRustSccache {
    cargoToml = ../../run-sgx/Cargo.toml;
    cargoExtraArgs = "--package=run-sgx --locked --offline";
    doCheck = false;

    nativeBuildInputs = lib.optionals (pkgs.hostPlatform.system == "x86_64-linux") [
      # ring crate build.rs
      pkgs.perl

      # aesm-client crate build.rs
      pkgs.protobuf
    ];
  };

  # Tiny enclave that exercises some basic SGX platform features.
  sgx-test = buildRustSgxPackage {
    cargoToml = ../../sgx-test/Cargo.toml;
    isSgx = true;
    isRelease = true;
  };

  # Convenience script to run `sgx-test`.
  run-sgx-test = pkgs.writeShellScriptBin "run-sgx-test" ''
    ${run-sgx}/bin/run-sgx ${sgx-test}/bin/sgx-test.sgxs --debug
  '';
}
