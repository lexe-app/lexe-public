{
  craneLib,
  sgx ? true,
}:
let
  cargoToml = ../../node-fake/Cargo.toml;
  crateInfo = craneLib.crateNameFromCargoToml cargoToml;
  sgxLabel = if sgx then "sgx" else "nosgx";
in
craneLib.buildPackage {
  src = craneLib.cleanCargoSource (craneLib.path ../..);

  pname = "${crateInfo.pname}-${sgxLabel}";
  version = crateInfo.version;

  cargoExtraArgs = "--package node-fake";

  doCheck = false;
}
