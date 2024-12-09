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
  # $ nix build --keep-going -L .#_dbg.systemLexePubPkgs.x86_64-linux.cargoVendorDir
  # ```
  gitDepOutputHashes = {
    "git+https://github.com/arik-so/rust-musig2?rev=6f95a05718cbb44d8fe3fa6021aea8117aa38d50#6f95a05718cbb44d8fe3fa6021aea8117aa38d50" = "sha256-+ksLhW4rXHDmi6xkPHrWAUdMvkm1cM/PBuJUnTt0vQk=";
    "git+https://github.com/lexe-app/axum-server?branch=lexe-v0.6.0-2024_10_11#ef4876f298eb963702704e5a6d976d304c145c1d" = "sha256-8jhdcSpI59Sf/Jg7zEI+QtJHSrhiWr5E+T2BnHD8Tjk=";
    "git+https://github.com/lexe-app/bdk?branch=lexe-v0.29.0-2024_10_11#b88e742008843707495de3634fb6bd5fe37e3da1" = "sha256-jSMYKVlrmgyFXjbWIwTpHB17SypqJn/TLne6uIdgYJ0=";
    "git+https://github.com/lexe-app/hyper-util?branch=lexe-v0.1.5-2024_10_11#5e6597befadd773ff7616248eb09d60339312bc1" = "sha256-QNhWHakQBKdYXMAnmWhgXyyg3LulgqYCWZggjl7tm7s=";
    "git+https://github.com/lexe-app/hyper?branch=lexe-v0.14.28-2024_10_11#dcb0ca215b6873b5966de529e017fb0e7412fb2e" = "sha256-pNBH2Ct6kzIwWWnHAMj4oPrrIMCLkcqxzM6JJ0MyqXo=";
    "git+https://github.com/lexe-app/mio?branch=lexe-v0.8.11-2024_10_11#84142417f08a9100114f4bda12931a481c1024d6" = "sha256-uYoek4uKq5Yxs0GARttet6lJ0u3xIyxkaLoHJaufJu0";
    "git+https://github.com/lexe-app/reqwest?branch=lexe-v0.11.26-2024_10_11#50cc5e16ff22c3f657edd7e4d9e6001fc55a69be" = "sha256-V5EiuZI2839ySHTre6cNNyl00x0O7Ukv3l9VHIBmpj8=";
    "git+https://github.com/lexe-app/reqwest?branch=lexe-v0.12.1-2024_10_11#2158f1ce3ef0df20fb646571f61caf8ba5a5b4ca" = "sha256-ADMgL4ivs63GJOp3OPQ4PWJslkA8w7/qtb40gYeIwP4=";
    "git+https://github.com/lexe-app/ring?branch=lexe-v0.16.20-2024_10_11#46842781024ab26ae8d4a77ac13153bd5ec013e3" = "sha256-LhbkszM16JzoucXH3vewzSn4WE+q/Zo1aCAdlqmh+BI=";
    "git+https://github.com/lexe-app/ring?branch=lexe-v0.17.8-2024_10_11#db1b9833cf8f80b6eb3445857846978497d80e66" = "sha256-Cw/yD0ebBhTUX7yQftHP0nNtm1bX626wTPeL2cC4wDw=";
    "git+https://github.com/lexe-app/rust-esplora-client?branch=lexe-v0.10.0-2024_11_14#aab0b6a230e4b27ad83e1d92ba00c4df1a05ea53" = "sha256-YzpAxKpRsgneaqvQvoTZMDDv5QRruhaIliTgvemmcUA=";
    "git+https://github.com/lexe-app/rust-lightning?branch=lexe-v0.0.125-2024_11_14#752c2e52227729cfb9c6172c96fcd00ef55d8db8" = "sha256-oTSjTEzMygmHaDQZ2aGFJBOssiqO1ux0LwOQVcJLsvw=";
    "git+https://github.com/lexe-app/rust-sgx?branch=lexe-30cfd65c-2024_08_29#30cfd65c2b537c4330d6702f3f692762cc7fe1a6" = "sha256-NVGQ+n0NY/UYHiav6BiSmkeFCeyKSDMtG6hPQ8MIQJ0=";
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
  blockstream-electrs = pkgs.callPackage ./blockstream-electrs.nix {
    inherit buildRustSccache vendorCargoDeps;
    rocksdb = pkgs.rocksdb_8_3;
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
  ];

  # Our flutter version
  flutter = pkgs.flutter324;

  # composeAndroidPackages =
  # { cmdLineToolsVersion ? "13.0"
  # , toolsVersion ? "26.1.1"
  # , platformToolsVersion ? "35.0.1"
  # , buildToolsVersions ? [ "34.0.0" ]
  # , includeEmulator ? false
  # , emulatorVersion ? "35.1.4"
  # , platformVersions ? []
  # , includeSources ? false
  # , includeSystemImages ? false
  # , systemImageTypes ? [ "google_apis" "google_apis_playstore" ]
  # , abiVersions ? [ "x86" "x86_64" "armeabi-v7a" "arm64-v8a" ]
  # , cmakeVersions ? [ ]
  # , includeNDK ? false
  # , ndkVersion ? "26.3.11579264"
  # , ndkVersions ? [ndkVersion]
  # , useGoogleAPIs ? false
  # , useGoogleTVAddOns ? false
  # , includeExtras ? []
  # , repoJson ? ./repo.json
  # , repoXmls ? null
  # , extraLicenses ? []
  # }:
  androidSdkComposition = pkgsUnfree.androidenv.composeAndroidPackages rec {
    abiVersions = ["armeabi-v7a" "arm64-v8a"];
    platformVersions = [
      "34" # lexe
      "31" # app_links
    ];
    buildToolsVersions = [
      "30.0.3" # gradle android plugin seems to want this?
    ];
    includeNDK = true;
    ndkVersion = "26.3.11579264";
    ndkVersions = [
      ndkVersion # lexe
      "23.1.7779620" # flutter_zxing
    ];
    cmakeVersions = ["3.18.1"]; # flutter_zxing
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
