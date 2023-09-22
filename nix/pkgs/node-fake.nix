{
  stdenv,
  lib,
  perl,
  craneLib,
  darwin,
  sgx ? true,
}: let
  cargoToml = ../../node-fake/Cargo.toml;
  crateInfo = craneLib.crateNameFromCargoToml {cargoToml = cargoToml;};
  sgxLabel =
    if sgx
    then "sgx"
    else "nosgx";
in
  craneLib.buildPackage {
    src = craneLib.cleanCargoSource (craneLib.path ../..);

    pname = "${crateInfo.pname}";
    version = "${crateInfo.version}-${sgxLabel}";

    cargoExtraArgs = builtins.concatStringsSep " " (
      ["--package=node-fake"]
      ++ (lib.optionals sgx ["--target=x86_64-fortanix-unknown-sgx"])
    );

    nativeBuildInputs = [
      # `ring` uses `perl` in its build.rs
      perl
    ];

    buildInputs = []
    ++ lib.optionals stdenv.isDarwin [
      # `ring` uses Security.framework rng on apple platforms
      darwin.apple_sdk.frameworks.Security
    ];

    doCheck = false;
  }
