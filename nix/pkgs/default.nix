# lexe public monorepo nix packages set
{
  lib,
  pkgs,
  pkgsUnfree,
  crane,
  fenixPkgs,
  lexePubLib,
}: rec {
  # cargo workspace Cargo.toml & Cargo.lock info
  workspaceRoot = ../..;
  workspaceToml = workspaceRoot + "/Cargo.toml";
  workspaceLock = workspaceRoot + "/Cargo.lock";
  workspaceLockParsed = builtins.fromTOML (builtins.readFile workspaceLock);
  workspaceTomlParsed = builtins.fromTOML (builtins.readFile workspaceToml);
  workspaceVersion = workspaceTomlParsed.workspace.package.version;

  # `fenix` rust toolchains need patching on macOS to work properly inside the
  # build sandbox.
  patchFenixRustToolchainIfMacOS = fenixToolchainUnpatched: let
    isDarwin = pkgs.targetPlatform.isDarwin;
  in
    # non-macOS doesn't need patching
    if !isDarwin
    then fenixToolchainUnpatched
    else
      # - On macOS, we need to patch `cargo` so it uses dynamic libs from
      #   nixpkgs. Otherwise it doesn't work in the sandbox.
      # - On macOS, we almost always need `libiconv` in any compiled binary.
      #   Add it as a "propagated" dep so we don't have to keep including it
      #   manually.
      # TODO(phlip9): upstream these changes
      fenixToolchainUnpatched.overrideAttrs (super: {
        # All darwin targets need libiconv
        depsTargetTargetPropagated = lib.optional pkgs.targetPlatform.isDarwin pkgs.pkgsTargetTarget.iconv;

        buildCommand = ''
          ${lib.optionalString pkgs.hostPlatform.isDarwin ''
            # darwin.cctools provides 'install_name_tool'
            export PATH="$PATH:${pkgs.darwin.cctools}/bin"
          ''}

          ${super.buildCommand}

          ${lib.optionalString pkgs.hostPlatform.isDarwin ''
            # Patch libcurl and libiconv so they use nixpkgs versions
            install_name_tool \
              -change "/usr/lib/libcurl.4.dylib" "${pkgs.curl.out}/lib/libcurl.4.dylib" \
              -change "/usr/lib/libiconv.2.dylib" "${pkgs.iconv.out}/lib/libiconv.2.dylib" \
              "$out/bin/cargo"
          ''}

          mkdir -p "$out/nix-support"
          [[ -z "$depsTargetTargetPropagated" ]] || echo "$depsTargetTargetPropagated " > $out/nix-support/propagated-target-target-deps
        '';
      });

  # Instantiate the rust toolchain from our `rust-toolchain.toml`.
  rustLexeToolchain = let
    fenixToolchainUnpatched = fenixPkgs.combine [
      fenixPkgs.stable.rustc
      fenixPkgs.stable.cargo
      fenixPkgs.targets.x86_64-fortanix-unknown-sgx.stable.rust-std
    ];

    # make fenix Rust work in build sandbox on macOS
    fenixToolchain = patchFenixRustToolchainIfMacOS fenixToolchainUnpatched;

    # HACK: get the actual rustc version from the fenix toolchain dl url
    # ex: `url = "https://static.rust-lang.org/dist/2024-08-08/cargo-1.80.1-x86_64-unknown-linux-gnu.tar.gz"`
    #     `dlFile = "cargo-1.80.1-x86_64-unknown-linux-gnu.tar.gz"`
    url = fenixPkgs.stable.cargo.src.url;
    dlFile = builtins.baseNameOf url;
    fenixToolchainVersion = builtins.elemAt (builtins.split "-" dlFile) 2;

    # parse our `rust-toolchain.toml` file and get the expected version
    rustToolchainToml = builtins.fromTOML (builtins.readFile ../../rust-toolchain.toml);
    rustToolchainVersion = rustToolchainToml.toolchain.channel;
  in
    # assert that the fenix stable toolchain uses our expected version
    assert lib.assertMsg (fenixToolchainVersion == rustToolchainVersion) ''
      The stable rust toolchain from fenix doesn't match rust-toolchain.toml:
      |
      |
      |           fenix stable: ${fenixToolchainVersion}
       `>  rust-toolchain.toml: ${rustToolchainVersion}

      Suggestion: update rust-toolchain.toml with `channel = "${rustToolchainVersion}"`.
    ''; fenixToolchain;

  # `crane` cargo builder instantiated with our rust toolchain settings.
  craneLib = (crane.mkLib pkgs).overrideToolchain rustLexeToolchain;

  # workspace source directory, cleaned of anything not needed to build rust
  # code
  fileset = lib.fileset;
  srcRust = fileset.toSource {
    root = workspaceRoot;
    fileset = fileset.unions [
      # sort by frequency
      (fileset.fileFilter
        (
          file:
            file.hasExt "rs"
            || file.name == "Cargo.toml"
            || file.hasExt "der"
        )
        workspaceRoot)
      ../../.cargo/config.toml
      ../../Cargo.lock
    ];
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
  # $ nix build --keep-going -L .#_dbg.systemLexePubPkgs.x86_64-linux.cargoVendorDir
  # ```
  gitDepOutputHashes = {
    "git+https://github.com/arik-so/rust-musig2?rev=6f95a05718cbb44d8fe3fa6021aea8117aa38d50#6f95a05718cbb44d8fe3fa6021aea8117aa38d50" = "sha256-+ksLhW4rXHDmi6xkPHrWAUdMvkm1cM/PBuJUnTt0vQk=";
    "git+https://github.com/lexe-app/axum-server?branch=lexe-v0.6.0-2024_10_11#ef4876f298eb963702704e5a6d976d304c145c1d" = "sha256-8jhdcSpI59Sf/Jg7zEI+QtJHSrhiWr5E+T2BnHD8Tjk=";
    "git+https://github.com/lexe-app/hyper-util?branch=lexe-v0.1.11-2025_05_15#bce9222cda7438c823f07c195d889ea3933044d2" = "sha256-HkxFi+kJsVEBebhbvfVkj2BlHu2HzdGhdmCO0IyMcJA=";
    "git+https://github.com/lexe-app/mio?branch=lexe-v0.8.11-2024_10_11#84142417f08a9100114f4bda12931a481c1024d6" = "sha256-uYoek4uKq5Yxs0GARttet6lJ0u3xIyxkaLoHJaufJu0";
    "git+https://github.com/lexe-app/reqwest?branch=lexe-v0.12.1-2024_10_11#2158f1ce3ef0df20fb646571f61caf8ba5a5b4ca" = "sha256-ADMgL4ivs63GJOp3OPQ4PWJslkA8w7/qtb40gYeIwP4=";
    "git+https://github.com/lexe-app/ring?branch=lexe-v0.16.20-2024_10_11#46842781024ab26ae8d4a77ac13153bd5ec013e3" = "sha256-LhbkszM16JzoucXH3vewzSn4WE+q/Zo1aCAdlqmh+BI=";
    "git+https://github.com/lexe-app/ring?branch=lexe-v0.17.8-2025_05_15#d33bd3d43eb277ca79a62fd1e0dd631d0bb53314" = "sha256-NH+bdZN1cFDQ9G4nDx4VeUFlT4G96obZlMTDaNqV0qs=";
    "git+https://github.com/lexe-app/rust-bip39?branch=lexe-v2.1.0-2025_06_12#81bdf38b89ea9542c7da849a9bba262bcb7cce34" = "sha256-0hvKvNNokzXHPNZmGEN2oqhc15khCrfsiGxDEfb3FFY=";
    "git+https://github.com/lexe-app/rust-esplora-client?branch=lexe-v0.12.0-2025_06_12#3fae9cdd82ce36aca6950a5614536de98f466a69" = "sha256-kDdRH6eXljD8gULyVOQUqYo51UgNmoMxZOTHyiLuvoo=";
    "git+https://github.com/lexe-app/rust-lightning?branch=lexe-v0.1.5-2025_10_06#51441b6765af6f87f20a4c13e0d3c778630375e0" = "sha256-rTRZE1njIPueSbV4az+p7IDhQu31h9jXowMxlXhnq3I=";
    "git+https://github.com/lexe-app/rust-sgx?branch=lexe-b77c27f2-2025_06_30#b77c27f24b38af183a6c0d7ef5df07cf01e84de0" = "sha256-6aBywD0ZZkhGEVMCIzHZ9uVW9QelujX/ojtRd1sw3mI=";
    "git+https://github.com/lexe-app/tokio?branch=lexe-v1.36.0-2024_10_11#f6d1d554668fe7530007e1a624e9d46d8755dfd6" = "sha256-ZUoZHJC9OZthqtFKu4WdrBgyr7QSKxoQCCUtcOc9kvU=";
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
    gitDepOutputHashes ? {},
    gitDepOutputs ? builtins.mapAttrs fetchGitDep gitDepOutputHashes,
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

            Try adding a new placeholder entry to `gitDepOutputHashes` in
            `public/nix/pkgs/default.nix` and re-running the build:

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
  blockstream-electrs = pkgs.blockstream-electrs;

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

  # Minimal `pkgs.mkShellNoCC` for `nix develop` that only
  #
  # 1. passes through `env`
  # 2. adds `packages` to the PATH.
  #
  # We use this for Apple dev toolchains, where nixpkgs's `stdenv` clobbers
  # several things that we need to get from the system instead.
  mkMinShell = {
    name,
    packages ? [],
    env ? {},
    shellHook ? "",
  }: let
    # Need to filter out attrNames used above so we don't accidentally clobber
    # TODO(phlip9): make this an assert
    envClean = builtins.removeAttrs env [
      "args"
      "builder"
      "name"
      "outputs"
      "packages"
      "shellHook"
      "stdenv"
      "system"
    ];
  in
    builtins.derivation ({
        name = name;
        system = pkgs.hostPlatform.system;
        builder = "${pkgs.bash}/bin/bash";
        outputs = ["out"];
        # The args are ignored in `nix develop`, but we need to create an output
        # to pass CI, which just builds the derivation.
        args = ["-c" "echo -n '' > $out"];

        # Explanation:
        #
        # `nix develop` builds a modified version of this derivation that changes
        # the derivation args to `get-env.sh`, a script packaged with `nix` itself.
        # It then builds the modified derivation, which runs `get-env.sh` with our
        # envs/packages/shellHook.
        #
        # 1. `get-env.sh` looks for a `$stdenv` env and runs `source $stdenv/setup`.
        #    -> we make this just dump all envs to $out
        # 2. `get-env.sh` looks for an `$outputs` env and for each output reads all
        #    the serialized envs from each line, returning them in a form that
        #    `nix develop` understands.
        #
        # So we add a `$stdenv` env which points to a directory containing a `setup`
        # bash script. This script then prints out the final envs to `$out`.
        stdenv = "${./min-stdenv}";

        packages = packages;
        shellHook = shellHook;
      }
      // envClean);

  #
  # app
  #

  # Rust with Android targets
  #
  # NOTE(phlip9): don't need to patch this toolchain since app builds don't work
  # inside the sandbox :'). Instead we just use a devShell.
  rustLexeToolchainAndroid = fenixPkgs.combine [
    fenixPkgs.stable.rustc
    fenixPkgs.stable.cargo
    # arm64 and arm-v7 cover 99.7% of all Android devices
    fenixPkgs.targets.aarch64-linux-android.stable.rust-std
    fenixPkgs.targets.armv7-linux-androideabi.stable.rust-std
    # but flutter seems to want x86_64...
    fenixPkgs.targets.x86_64-linux-android.stable.rust-std
  ];

  # Rust with iOS/macOS targets
  #
  # NOTE(phlip9): don't need to patch this toolchain since app builds don't work
  # inside the sandbox :'). Instead we just use a devShell.
  rustLexeToolchainiOSmacOS = fenixPkgs.combine [
    fenixPkgs.stable.rustc
    fenixPkgs.stable.cargo

    # TODO(phlip9): x86_64-apple-darwin?
    fenixPkgs.targets.aarch64-apple-darwin.stable.rust-std
    # iOS uses a different target for simulator vs real HW
    fenixPkgs.targets.aarch64-apple-ios.stable.rust-std
    fenixPkgs.targets.aarch64-apple-ios-sim.stable.rust-std
  ];

  # Our flutter version
  flutter = pkgs.flutter332;

  # composeAndroidPackages =
  # { cmdLineToolsVersion ? "latest",
  # , toolsVersion ? "latest",
  # , platformToolsVersion ? "latest",
  # , buildToolsVersions ? [ "latest" ],
  # , includeEmulator ? false,
  # , emulatorVersion ? "latest",
  # , minPlatformVersion ? null,
  # , maxPlatformVersion ? "latest",
  # , numLatestPlatformVersions ? 1,
  # , platformVersions ? ..,
  # , includeSources ? false,
  # , includeSystemImages ? false,
  # , systemImageTypes ? [ "google_apis" "google_apis_playstore" ],
  # , abiVersions ? [ "x86" "x86_64" "armeabi-v7a" "arm64-v8a" ],
  # , includeCmake ? stdenv.hostPlatform.isx86_64 || stdenv.hostPlatform.isDarwin,
  # , cmakeVersions ? [ "latest" ],
  # , includeNDK ? false,
  # , ndkVersion ? "latest",
  # , ndkVersions ? [ ndkVersion ],
  # , useGoogleAPIs ? false,
  # , useGoogleTVAddOns ? false,
  # , includeExtras ? [ ],
  # , repoJson ? ./repo.json,
  # , repoXmls ? null,
  # , extraLicenses ? [ ],
  # }:
  androidSdkComposition = pkgsUnfree.androidenv.composeAndroidPackages rec {
    abiVersions = ["armeabi-v7a" "arm64-v8a"];
    platformVersions = [
      "35" # lexe
      "34" # app_links, flutter_zxing -> camera_android_camerax
    ];
    buildToolsVersions = [
      "34.0.0" # gradle android plugin seems to want this?
    ];
    includeNDK = true;
    # TODO(phlip9): use `28.1.13356709` for 16KiB page size support
    ndkVersion = "27.0.12077973";
    ndkVersions = [
      ndkVersion # lexe, flutter_zxing
    ];
    cmakeVersions = ["3.22.1"]; # flutter_zxing
  };

  # Links all the toolchains/libs/bins/etc in our chosen `androidSdkComposition`
  # into a single derivation.
  androidSdk = androidSdkComposition.androidsdk;

  # Android envs
  ANDROID_SDK_ROOT = "${androidSdk}/libexec/android-sdk";
  ANDROID_HOME = ANDROID_SDK_ROOT;
  ANDROID_NDK_ROOT = "${ANDROID_SDK_ROOT}/ndk/${androidSdkComposition.ndk-bundle.version}";
  JAVA_HOME = "${pkgs.jdk17_headless.home}";

  # # The gradle version we're using.
  # # See: <app/android/gradle/wrapper/gradle-wrapper.properties>
  # gradle = pkgs.gradle_7;
}
