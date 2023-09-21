{
  lib,
  craneLib,
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

    pname = "${crateInfo.pname}-${sgxLabel}";
    version = crateInfo.version;

    cargoExtraArgs = builtins.concatStringsSep " " (
      ["--package=node-fake"]
      ++ (lib.optionals sgx ["--target=x86_64-fortanix-unknown-sgx"])
    );

    doCheck = false;
  }
