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
#     cargoExtraArgs = builtins.concatStringsSep " " (
#       ["--package=node-fake"]
#       ++ (lib.optionals sgx ["--target=x86_64-fortanix-unknown-sgx"])
#     );
#
#     # nativeBuildInputs = [
#     #   # `ring` uses `perl` in its build.rs
#     #   perl
#     # ];
#     #
#     # buildInputs =
#     #   []
#     #   ++ lib.optionals stdenv.isDarwin [
#     #     # `ring` uses Security.framework rng on apple platforms
#     #     darwin.apple_sdk.frameworks.Security
#     #   ];
#
#     doCheck = false;
#   }

# explicit version for debugging
{
  stdenvNoCC,
  lib,
  rustLexeToolchain,
  craneLib,
  tree, # TODO: remove
  jq,
  sgx ? true,
}: let
  sgxLabel =
    if sgx
    then "sgx"
    else "nosgx";

  src = craneLib.cleanCargoSource (craneLib.path ../..);
in
  stdenvNoCC.mkDerivation {
    src = src;

    pname = "node-fake";
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

      jq # used by installFromCargoBuildLogHook
    ];

    buildPhase = ''
      runHook preBuild

      # echo "=== .cargo/config.toml ==="
      # cat .cargo/config.toml

      # echo "=== .cargo-home/config.toml ==="
      # cat .cargo-home/config.toml

      cargo --version
      rustc --version

      cargoBuildLog=$(mktemp cargoBuildLogXXXX.json)
      cargo build \
        --verbose \
        --locked \
        --offline \
        --profile=release-sgx \
        --package=node-fake \
        --target=x86_64-fortanix-unknown-sgx \
        --message-format json-render-diagnostics \
        > "$cargoBuildLog"

      # cat "$cargoBuildLog"

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
      # ${tree}/bin/tree $out
      sha256sum $out/bin/node-fake
      stat --format='Size: %s' $out/bin/node-fake
    '';
  }
