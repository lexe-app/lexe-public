{
  stdenv,
  lib,
  llvmPackages,
  craneLib,
  darwin,
  perl,
  protobuf,
  # this should probably be encapsulated into a new "stdenv" targetting
  # `x86_64-fortanix-unknown-sgx`, but I'm not quite sure how to do that yet.
  isSgx ? true,
  isRelease ? true,
  # enable full, verbose build logs
  isVerbose ? false,
}: let
  cargoToml = ../../node-fake/Cargo.toml;
  crateInfo = craneLib.crateNameFromCargoToml {cargoToml = cargoToml;};

  # include C header files and hard-coded CA certs
  miscFilter = path: type: (
    let
      pathStr = builtins.toString path;
      fileName = builtins.baseNameOf pathStr;
    in
      (lib.hasSuffix ".h" fileName) || (lib.hasSuffix ".der" fileName)
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

    nativeBuildInputs = [
      # ring crate build.rs
      perl
      # # aesm-client crate build.rs
      # protobuf
    ];

    buildInputs =
      []
      ++ lib.optionals (!isSgx && stdenv.isDarwin) [
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
      "${src}/sgx-libc-shim/include"
    ];

    # We use `cargo`'s built-in stripping via the `release-sgx` profile.
    dontStrip = isSgx;
    # The release binary is statically linked so patchelf is not necessary.
    dontPatchELF = isSgx;
    dontAutoPatchelf = isSgx;
    dontPatchShebangs = isSgx;
  };

  # TODO: figure out how
  depsOnly = craneLib.buildDepsOnly commonPackageArgs;
in
  craneLib.buildPackage (
    commonPackageArgs
    // {
      cargoArtifacts = depsOnly;

      # print out the binary hash and size for debugging
      postFixup = ''
        sha256sum $out/bin/${crateInfo.pname}
        stat --format='Size: %s' $out/bin/${crateInfo.pname}
      '';
    }
  )
