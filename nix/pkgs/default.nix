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
  workspaceLockParsed = builtins.fromTOML (builtins.readFile workspaceLock);
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

  # To better guarantee reproducibility, each git dependency needs to pin the
  # exact output hash of its _unzipped and extracted_ git repo directory. This
  # is the moral equivalent of each flake input in `flake.lock` committing to both
  # the revision _and_ the final `narHash`.
  #
  # Previously, we used `craneLib.vendorCargoDeps` directly with no `outputHashes`,
  # which uses the eval-time `builtins.fetchgit` function. This was super
  # convenient, as we didn't have to track these extra hashes, only what we were
  # already tracking with the Cargo.lock. However, we experienced reproducibility
  # failures across platforms and `nix` versions, where `builtins.fetchgit` would
  # return different /nix/store/... paths. These paths show up indirectly in the
  # final binary via cargo hashing the full path when computing each crate hash.
  # Hence this extra annoying workaround.
  #
  # Quickly extract all git deps from Cargo.lock w/ placeholder hashes:
  # ```
  # $ nix-instantiate --eval \
  #     -E '{json}: builtins.fromJSON json' \
  #     --argstr json "$( \
  #       toml2json Cargo.lock \
  #       | jq -crS '.package | map(.source | select(. != null and startswith("git+")) | { key: ., value: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" }) | from_entries' \
  #     )"
  # ```
  #
  # Get all updated output hashes:
  # ```
  # $ nix build --keep-going -L .#cargoVendorDir
  # ```
  gitDepOutputHashes = {
    "git+https://github.com/arik-so/rust-musig2?rev=cff11e3#cff11e3b1af1691f721a120dc6acb921afa31f89" = "sha256-++1x7uHHR7KEhl8LF3VywooULiTzKeDu3e+0/c/8p9Y=";
    "git+https://github.com/lexe-app/axum-server?branch=lexe-v0.6.0-2024_05_20#25a7f52c0a1ba91f4e2ad80fff649fce377483c4" = "sha256-iA0uAKXlC+x7l9W8CqTsROjAqifqvnmBKnquu2NUgFc=";
    "git+https://github.com/lexe-app/bdk?branch=lexe-v0.29.0-2024_07_24#9868657663ec65a6be585d7721421fb877b11fe4" = "sha256-38SzJDSskwXJGfU2Wj8YPrsUXjAUQ5Vqj76yUK11/T8=";
    "git+https://github.com/lexe-app/hyper-util?branch=lexe-v0.1.5-2024_06_03#c817db0b44c11ef194e19b51da6451eb1d328f2d" = "sha256-wX1qxhM9h7loBqU7quvy1aXbygO4ZWiG+peNV9oMjRQ=";
    "git+https://github.com/lexe-app/hyper?branch=lexe-v0.14.28-2024_03_08#4d072553660a707b7a22b8e4b811e0458e865895" = "sha256-qoQaq/EXBvEE1ptwgdc0m5tupWUG0wLC+v+UQZqTmjs=";
    "git+https://github.com/lexe-app/mio?branch=lexe-v0.8.11-2024_07_01#dc3cdb65d392dc67bc14dac6fe0d53e16e65d009" = "sha256-Q52i0M9zZWec5JJhh7Rf2v3HX6fTUnr8n2h7sj2B2Ec=";
    "git+https://github.com/lexe-app/reqwest?branch=lexe-v0.11.26-2024_03_12#1589a52107374fbe14651a1adce2c4463ebf410f" = "sha256-WjQasUfVgH9436d3PQQoHX52RUPCHg92Zz0asSIjfeI=";
    "git+https://github.com/lexe-app/reqwest?branch=lexe-v0.12.1-2024_03_22#912562f0f2399cb29d8e0456d65732c641382683" = "sha256-cBZOjms6xj/yQ8KDi5sQKC0NVGdC5E7pkU6k7cKcL9A=";
    "git+https://github.com/lexe-app/ring?branch=lexe-v0.16.20-2023_09_26#6aad00356d5eea8d4c8a00e7f53775e9fedf53be" = "sha256-9CQjWJ0Fk8twGRYv2mMoEMFinrM/2WVQ+X2fV/59vXI=";
    "git+https://github.com/lexe-app/ring?branch=lexe-v0.17.8-2024_03_21#56c92f51cc0b5dc7ecbfd3c61ed0bbd53546cca6" = "sha256-YljqthNfkFxcIg8Cl2iCIqMG80IMl8hF+Un0rdH+0hY=";
    "git+https://github.com/lexe-app/rust-lightning?branch=lexe-v0.0.123-2024_07_25#f7bbb05c7ee1f532a2eb9e206320ad80d3f752aa" = "sha256-0UJFW/YvS4k1Sh7Lln2xSE8Cx3Qpgl2Y++UA3amWDmE=";
    "git+https://github.com/lexe-app/rust-sgx?branch=lexe-b6f02626-2024_06_28#8840ad93cd1b56dac1d6c24fce28cc3f5b6899a6" = "sha256-yIttRviwnc9vHCkbIk7iTVo/Yhsul4wU1Xov7FjSZVs=";
    "git+https://github.com/lexe-app/tokio?branch=lexe-v1.36.0-2024_07_01#3df5631ecea5ffcafd3d6fd1a141629a1630a534" = "sha256-LmlC43TdO3iUImHmix/M8YvVdG0jqeJeadqgXFsPiOE=";
  };

  # Quickly fetch a gitdep with its output hash using `pkgs.fetchFromGitHub`.
  fetchGitDep = source: hash: let
    inherit (builtins) elemAt match substring;
    matches = match "git\\+https://github.com/([^/]+)/([^/?]+)\\?.*#([0-9a-f]{40})" source;
    owner = elemAt matches 0;
    repo = elemAt matches 1;
    rev = elemAt matches 2;
    shortRev = substring 0 8 rev;
  in
    pkgs.fetchFromGitHub {
      name = "${repo}-${shortRev}-source";
      owner = owner;
      repo = repo;
      rev = rev;
      hash = hash;

      # These are critical to avoid known reproducibility hazards.
      # I believe these are off by default, but let's be explicit.
      fetchSubmodules = false;
      forceFetchGit = false;
    };

  # This is `gitDepOutputHashes` but each value is the derivation containing
  # the fetched git dep directory.
  gitDepOutputs = builtins.mapAttrs fetchGitDep gitDepOutputHashes;

  # for debugging fetcher reproducibility issues...
  # $ nix build --repair --keep-failed --show-trace .#_dbg.systemLexePubPkgs.x86_64-linux._gitDepOutputsDebugging.ring-6aad0035-source
  _gitDepOutputsDebugging = builtins.listToAttrs (builtins.map (drv: {
    name = drv.name;
    value = drv;
  }) (builtins.attrValues gitDepOutputs));

  # A function to vendor all cargo dependencies from a Cargo.lock file.
  vendorCargoDeps = {
    cargoLock ? throw "Requires oneof `cargoLock`, `cargoLockContents`, `cargoLockParsed`",
    cargoLockContents ? builtins.readFile cargoLock,
    cargoLockParsed ? builtins.fromTOML cargoLockContents,
    gitDepOutputs ? {},
    gitDepOutputHashes ? {},
  }:
    craneLib.vendorMultipleCargoDeps {
      cargoConfigs = []; # only used if we have custom registries
      cargoLockParsedList = [cargoLockParsed];
      outputHashes = gitDepOutputHashes;
      overrideVendorCargoPackage = _ps: drv: drv;
      overrideVendorGitCheckout = ps: drv: let
        # A git-dep [[package]] entry in the `Cargo.lock`
        package = builtins.head ps;
      in
        if !(gitDepOutputs ? ${package.source})
        then
          builtins.throw ''
            Error: missing an output hash for this cargo git dependency: ${builtins.toJSON package}

            Try adding a new placeholder entry to `gitDepOutputHashes` in `nix/pkgs/default.nix`
            and re-running the build:

            gitDepOutputHashes = {
              # ...
              "${package.source}" = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
              # ...
            };
          ''
        else drv.overrideAttrs {src = gitDepOutputs.${package.source};};
    };

  # Download and vendor all cargo deps from the workspace Cargo.lock into the
  # nix store.
  cargoVendorDir = vendorCargoDeps {
    cargoLockParsed = workspaceLockParsed;
    gitDepOutputs = gitDepOutputs;
    gitDepOutputHashes = gitDepOutputHashes;
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
  rustSgxCargoSource = let
    inherit (builtins) attrNames filter head match;
    gitDepSources = attrNames gitDepOutputHashes;
  in
    head (filter (source: (match ".*/lexe-app/rust-sgx\\?.*" source) != null) gitDepSources);
  rustSgxSrc = gitDepOutputs.${rustSgxCargoSource};
  rustSgxCargoVendorDir = vendorCargoDeps {
    cargoLock = rustSgxSrc + "/Cargo.lock";
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
