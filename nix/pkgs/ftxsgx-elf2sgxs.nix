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
  lib,
  craneLib,
}: let
  # Parse the git revision of a git dependency from a `Cargo.lock` file.
  # ex return: "4aa8f13487c772dd4d24b7cc54bd2d5432803f7a"
  parseCargoLockGitDep = {
    # ex: `builtins.readFile ./Cargo.lock`
    cargoLockContents,
    # ex: "https://github.com/lexe-app/rust-sgx"
    githubUrl,
  }: let
    inherit (builtins) elemAt filter head match;
    inherit (lib) splitString escapeRegex;

    lines = splitString "\n" cargoLockContents;
    escapedUrl = escapeRegex githubUrl;
    pattern = "source = \"git\\+${escapedUrl}\\?branch=([^#]+)#([0-9a-f]{40})\"";
    firstMatchingLine = head (filter (line: (match pattern line) != null) lines);
    matches = match pattern firstMatchingLine;
  in {
    url = "${githubUrl}.git";
    ref = elemAt matches 0;
    rev = elemAt matches 1;
  };

  rustSgxRepo = parseCargoLockGitDep {
    githubUrl = "https://github.com/lexe-app/rust-sgx";
    cargoLockContents = builtins.readFile ../../Cargo.lock;
  };

  rustSgxSrc = builtins.fetchGit {
    inherit (rustSgxRepo) url ref rev;
    shallow = true;
  };

  crateInfo = craneLib.crateNameFromCargoToml {
    cargoToml = "${rustSgxSrc}/intel-sgx/fortanix-sgx-tools/Cargo.toml";
  };
in
  craneLib.buildPackage {
    src = rustSgxSrc;

    pname = "ftxsgx-elf2sgxs";
    version = crateInfo.version;
    doCheck = false;
    cargoArtifacts = null;

    cargoExtraArgs = builtins.concatStringsSep " " [
      "--offline"
      "--locked"
      "--package=fortanix-sgx-tools"
      "--bin=ftxsgx-elf2sgxs"
      "--no-default-features"
    ];
  }
