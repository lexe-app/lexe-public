# This package provides the `ftxsgx-elf2sgxs` tool, which converts an ELF binary
# targetting `x86_64-fortanix-unknown-sgx` into a canonical `.sgxs` file. These
# `.sgxs` files are special, as they exactly mirror the memory layout of the
# enclave as it's initially loaded into memory and measured (hashed) by the SGX
# platform.
#
# That means we can literally run `sha256sum` on the `.sgxs` file and get the
# exact same enclave measurement as the SGX platform, without having to actually
# load the enclave.
{
  buildRustSccache,
  rustSgxSrc,
  rustSgxCargoVendorDir,
}:
buildRustSccache {
  cargoToml = "${rustSgxSrc}/intel-sgx/fortanix-sgx-tools/Cargo.toml";
  src = rustSgxSrc;
  cargoVendorDir = rustSgxCargoVendorDir;

  pname = "ftxsgx-elf2sgxs";
  doCheck = false;
  buildForLexeInfra = false;

  cargoExtraArgs = builtins.concatStringsSep " " [
    "--offline"
    "--locked"
    "--package=fortanix-sgx-tools"
    "--bin=ftxsgx-elf2sgxs"
    "--no-default-features"
  ];
}
