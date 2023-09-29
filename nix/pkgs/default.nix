# lexe public monorepo nix packages set
{
  pkgs,
  crane,
}: rec {
  # A rust toolchain setup from our `rust-toolchain.toml` settings.
  rustLexeToolchain =
    pkgs.rust-bin.fromRustupToolchainFile ../../rust-toolchain.toml;

  # `crane` cargo builder instantiated with our rust toolchain settings.
  craneLib = (crane.mkLib pkgs).overrideToolchain rustLexeToolchain;

  # Use the latest clang/llvm for cross-compiling SGX.
  llvmPackages = pkgs.llvmPackages_latest;

  # Shim a small set of libc fns so we can cross-compile SGX without glibc.
  sgx-libc-shim = pkgs.callPackage ./sgx-libc-shim.nix {};

  # Converts a compiled `x86_64-fortanix-unknown-sgx` ELF binary into
  # a `.sgxs` enclave file.
  ftxsgx-elf2sgxs = pkgs.callPackage ./ftxsgx-elf2sgxs.nix {
    craneLib = craneLib;
  };

  # A hook that runs `ftxsgx-elf2sgxs` on the output binary in the
  # `postFixup` phase.
  elf2sgxsFixupHook = pkgs.callPackage ./elf2sgxsFixupHook.nix {
    ftxsgx-elf2sgxs = ftxsgx-elf2sgxs;
  };

  # User's node SGX enclave
  node-release-sgx = pkgs.callPackage ./node.nix {
    isSgx = true;
    isRelease = true;
    inherit craneLib llvmPackages elf2sgxsFixupHook sgx-libc-shim;
  };
  node-debug-sgx = pkgs.callPackage ./node.nix {
    isSgx = true;
    isRelease = false;
    inherit craneLib llvmPackages elf2sgxsFixupHook sgx-libc-shim;
  };
  node-release-nosgx = pkgs.callPackage ./node.nix {
    isSgx = false;
    isRelease = true;
    inherit craneLib llvmPackages elf2sgxsFixupHook sgx-libc-shim;
  };
  node-debug-nosgx = pkgs.callPackage ./node.nix {
    isSgx = false;
    isRelease = false;
    inherit craneLib llvmPackages elf2sgxsFixupHook sgx-libc-shim;
  };
}
