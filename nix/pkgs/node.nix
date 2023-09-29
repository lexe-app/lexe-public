# Reproducibly build the user `node` enclave.
{
  #
  # nixpkgs
  #
  darwin,
  lib,
  llvmPackages,
  perl,
  protobuf,
  stdenvNoCC,
  #
  # lexe packages
  #
  craneLib,
  elf2sgxsFixupHook,
  sgx-libc-shim,
  #
  # options
  #
  isRelease ? true,
  # this should probably be encapsulated into a new "stdenv" targetting
  # `x86_64-fortanix-unknown-sgx`, but I'm not quite sure how to do that yet.
  isSgx ? true,
  # enable full, verbose build logs
  isVerbose ? false,
}: let
  cargoToml = ../../node/Cargo.toml;
  cargoTomlContents = builtins.readFile cargoToml;
  crateInfo = craneLib.crateNameFromCargoToml {cargoTomlContents = cargoTomlContents;};

  # include hard-coded CA certs
  miscFilter = path: type: (
    let
      pathStr = builtins.toString path;
      fileName = builtins.baseNameOf pathStr;
    in (lib.hasSuffix ".der" fileName)
  );

  # strip all files not needed for Rust build
  srcFilter = path: type:
    (craneLib.filterCargoSources path type) || (miscFilter path type);

  src = lib.cleanSourceWith {
    src = lib.cleanSource ../..;
    filter = srcFilter;
  };

  commonPackageArgs = {
    src = src;

    pname = crateInfo.pname;
    version = crateInfo.version;

    # print cc full args list
    NIX_DEBUG = isVerbose;
    strictDeps = true;
    doCheck = false;

    nativeBuildInputs =
      [
        # ring crate build.rs
        perl
      ]
      ++ lib.optionals isSgx [
        # aesm-client crate build.rs
        protobuf
      ];

    buildInputs =
      []
      ++ lib.optionals (!isSgx && stdenvNoCC.isDarwin) [
        # ring crate uses Security.framework rng on apple platforms
        darwin.apple_sdk.frameworks.Security
      ];

    cargoExtraArgs = builtins.concatStringsSep " " (
      ["--offline" "--locked" "--package=${crateInfo.pname}"]
      ++ (lib.optionals isSgx ["--target=x86_64-fortanix-unknown-sgx"])
      ++ (lib.optionals isVerbose ["-vv"])
    );

    CARGO_PROFILE =
      if (isRelease && isSgx)
      then "release-sgx"
      else if isRelease
      then "release"
      else "dev";

    # Use llvm toolchain for sgx since it's significantly better for
    # cross-compiling.
    #
    # NOTE: `CC_*` and `CFLAGS_*` are used `cc-rs` in the `ring` build script,
    #       while `CARGO_TARGET_*` is used by `cargo` itself.
    CC_x86_64-fortanix-unknown-sgx = "${llvmPackages.clang-unwrapped}/bin/clang";
    CARGO_TARGET_X86_64_FORTANIX_UNKNOWN_SGX_LINKER = "${llvmPackages.lld}/bin/ld.lld";
    CFLAGS_x86_64-fortanix-unknown-sgx = let
      clang-unwrapped = llvmPackages.clang-unwrapped;
      clangVersion = lib.versions.major clang-unwrapped.version;
      clangResourceDir = "${clang-unwrapped.lib}/lib/clang/${clangVersion}/include";
    in [
      # The base includes, like `stdint.h`, `stddef.h`, and CPU intrinsics.
      "-isystem"
      "${clangResourceDir}"
      # libc shims -- the shimmed fn impls are provided by `rust-sgx/rs-libc`
      "-isystem"
      "${sgx-libc-shim}/include"
    ];

    # We use `cargo`'s built-in stripping via the `release-sgx` profile.
    dontStrip = isSgx;
    # The release binary is statically linked so patchelf is not necessary.
    dontPatchELF = isSgx;
    dontAutoPatchelf = isSgx;
    dontPatchShebangs = isSgx;
  };

  depsOnly = craneLib.buildDepsOnly commonPackageArgs;
in
  craneLib.buildPackage (
    commonPackageArgs
    // {
      cargoArtifacts = depsOnly;

      nativeBuildInputs =
        commonPackageArgs.nativeBuildInputs
        ++ (lib.optionals isSgx [
          (elf2sgxsFixupHook {
            cargoTomlContents = cargoTomlContents;
            isRelease = isRelease;
          })
        ]);

      postFixup = ''
        echo "ELF binary hash: $(sha256sum < $out/bin/${crateInfo.pname})"
        echo "ELF binary size: $(stat --format='%s' $out/bin/${crateInfo.pname})"
      '';
    }
  )
