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

  # Converts a compiled `x86_64-fortanix-unknown-sgx` ELF binary into
  # a `.sgxs` enclave file.
  ftxsgx-elf2sgxs = pkgs.callPackage ./ftxsgx-elf2sgxs.nix {
    inherit craneLib lexePubLib;
  };

  # A hook that runs `ftxsgx-elf2sgxs` on the output binary in the
  # `postFixup` phase.
  elf2sgxsFixupHook = pkgs.callPackage ./elf2sgxsFixupHook.nix {
    ftxsgx-elf2sgxs = ftxsgx-elf2sgxs;
  };

  # Generic builder for Rust SGX crates.
  buildRustSgxPackage = pkgs.callPackage ./buildRustSgxPackage.nix {
    inherit craneLib cargoVendorDir srcRust sgxCrossEnvBuildHook elf2sgxsFixupHook;
  };

  # Generic rust builder for non-SGX crates. Supports shared nix cargo
  # incremental build cache.
  buildRustIncremental = pkgs.callPackage ./buildRustIncremental.nix {
    inherit craneLib cargoVendorDir srcRust workspaceVersion;
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
  run-sgx = buildRustIncremental {
    cargoToml = ../../run-sgx/Cargo.toml;
    cargoExtraArgs = "-p run-sgx --bin run-sgx --locked --offline";

    nativeBuildInputs = lib.optionals (pkgs.hostPlatform.system == "x86_64-linux") [
      # aesm-client crate build.rs
      pkgs.protobuf
      # enclave-runner crate
      pkgs.pkg-config
    ];

    buildInputs =
      # enclave-runner crate
      lib.optional (pkgs.hostPlatform.system == "x86_64-linux") pkgs.openssl;
  };
}
