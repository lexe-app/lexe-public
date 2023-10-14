{
  #
  # nixpkgs
  #
  cmake,
  openssl,
  pkg-config,
  protobuf,
  #
  # lexePubPkgs
  #
  buildRustIncremental,
  rustSgxSrc,
  rustSgxCargoVendorDir,
}:
buildRustIncremental {
  cargoToml = "${rustSgxSrc}/intel-sgx/sgxs-tools/Cargo.toml";
  src = rustSgxSrc;
  cargoVendorDir = rustSgxCargoVendorDir;

  pname = "sgx-detect";
  doCheck = false;

  cargoExtraArgs = builtins.concatStringsSep " " [
    "--offline"
    "--locked"
    "--package=sgxs-tools"
    "--bin=sgx-detect"
    "--target=x86_64-unknown-linux-gnu"
  ];

  nativeBuildInputs = [
    pkg-config
    protobuf
    cmake
  ];
  buildInputs = [
    openssl
  ];

  meta = {
    platforms = ["x86_64-linux"];
  };
}
