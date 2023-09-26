# # simpler version
# # TODO: go back to this when I get everything working
# {
#   stdenv,
#   lib,
#   perl,
#   craneLib,
#   darwin,
#   sgx ? true,
# }: let
#   cargoToml = ../../node-fake/Cargo.toml;
#   crateInfo = craneLib.crateNameFromCargoToml {cargoToml = cargoToml;};
#   sgxLabel =
#     if sgx
#     then "sgx"
#     else "nosgx";
# in
#   craneLib.buildPackage {
#     src = craneLib.cleanCargoSource (craneLib.path ../..);
#
#     pname = "${crateInfo.pname}";
#     version = "${crateInfo.version}-${sgxLabel}";
#
#     # print cc full args list
#     NIX_DEBUG = true;
#
#     CARGO_PROFILE = "release-sgx";
#
#     cargoExtraArgs = builtins.concatStringsSep " " (
#       ["-vv" "--offline" "--locked" "--package=node-fake"]
#       ++ (lib.optionals sgx ["--target=x86_64-fortanix-unknown-sgx"])
#     );
#
#     nativeBuildInputs = [
#       # `ring` uses `perl` in its build.rs
#       perl
#     ];
#
#     buildInputs =
#       []
#       ++ lib.optionals stdenv.isDarwin [
#         # `ring` uses Security.framework rng on apple platforms
#         darwin.apple_sdk.frameworks.Security
#       ];
#
#     doCheck = false;
#
#     # We use `cargo`'s built-in stripping via the `release-sgx` profile.
#     dontStrip = true;
#     # The result binary is statically linked so patchelf is not necessary.
#     dontPatchELF = true;
#     dontAutoPatchelf = true;
#     dontPatchShebangs = true;
#   }

# explicit version for debugging
{
  lib,
  llvmPackages,
  rustLexeToolchain,
  craneLib,
  perl,
  jq,
  protobuf,
  sgx ? true,
}: let
  package = "node-fake";

  sgxLabel =
    if sgx
    then "sgx"
    else "nosgx";

  # include C header files and DER-encoded certs
  miscFilter = path: type: (
    let
      pathStr = builtins.toString path;
      fileName = builtins.baseNameOf pathStr;
    in
      (lib.hasSuffix ".h" fileName) || (lib.hasSuffix ".der" fileName)
  );

  srcFilter = path: type:
    (craneLib.filterCargoSources path type) || (miscFilter path type);

  src = lib.cleanSourceWith {
    src = lib.cleanSource ../..;
    filter = srcFilter;
  };
in
  llvmPackages.stdenv.mkDerivation {
    src = src;

    pname = "${package}";
    version = "0.1.0-${sgxLabel}";

    # A directory of vendored cargo sources which can be consumed without network
    # access. Directory structure should basically follow the output of `cargo vendor`.
    #
    # `nix` doesn't allow network access in the build sandbox (for good reason)
    # so we need this `craneLib` fn which consumes the `Cargo.lock` in nix-lang
    # and fetches the dependency sources in a reproducible way.
    cargoVendorDir = craneLib.vendorCargoDeps {src = src;};

    nativeBuildInputs = [
      rustLexeToolchain

      # prints any `cargo` invocations for debugging
      craneLib.cargoHelperFunctionsHook

      # points `CARGO_HOME` to `.cargo-home/config.toml`.
      # plus some other misc. things.
      craneLib.configureCargoCommonVarsHook
      # adds settings to `.cargo-home/config.toml` so `cargo` will pick up the
      # vendored deps.
      craneLib.configureCargoVendoredDepsHook
      # pretty magical; this moves the desired `cargo build` outputs from
      # `target/` -> `$out/`.
      craneLib.installFromCargoBuildLogHook
      craneLib.removeReferencesToVendoredSourcesHook

      # used by installFromCargoBuildLogHook
      jq

      # ring build.rs
      perl

      # aesm-client build.rs
      protobuf
    ];

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
    in
    [
      # The base includes, like `stdint.h`, `stddef.h`, and CPU intrinsics.
      "-isystem" "${clangResourceDir}"
      # libc shims -- the shimmed fn impls are provided by `rust-sgx/rs-libc`
      "-isystem" "${src}/sgx-libc-shim/include"
    ];

    buildPhase = ''
      runHook preBuild

      echo "source: ${src}"
      # ls -la "${src}"

      cargo --version
      rustc --version

      echo "SGX target toolchain"
      ${llvmPackages.clang-unwrapped}/bin/clang --version
      ${llvmPackages.lld}/bin/ld.lld --version

      cargoBuildLog=$(mktemp cargoBuildLogXXXX.json)
      cargo build \
        -vv \
        --locked \
        --offline \
        --profile=release-sgx \
        --package=${package} \
        --target=x86_64-fortanix-unknown-sgx \
        --message-format json-render-diagnostics \
        > "$cargoBuildLog"

      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall

      mkdir -p $out

      if [ -n "$cargoBuildLog" -a -f "$cargoBuildLog" ]; then
        installFromCargoBuildLog "$out" "$cargoBuildLog"
      else
        echo Missing "\$cargoBuildLog file"
        false
      fi

      runHook postInstall
    '';

    # We use `cargo`'s built-in stripping via the `release-sgx` profile.
    dontStrip = true;
    # The result binary is statically linked so patchelf is not necessary.
    dontPatchELF = true;
    dontAutoPatchelf = true;
    dontPatchShebangs = true;

    # print out the binary hash and size for debugging
    postFixup = ''
      sha256sum $out/bin/${package}
      stat --format='Size: %s' $out/bin/${package}
    '';
  }
