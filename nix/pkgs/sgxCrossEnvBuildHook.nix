# Setup the right env variable incantations for cross-compiling to SGX.
# Injects them into the `buildPhase` of the dependent crate builder.
{
  #
  # nixpkgs
  #
  lib,
  llvmPackages,
  makeSetupHook,
  writeShellScript,
  #
  # lexePkgs
  #
  sgx-libc-shim,
}: let
  lld = llvmPackages.lld;
  clang-unwrapped = llvmPackages.clang-unwrapped;
  clangVersion = lib.versions.major clang-unwrapped.version;
  clangResourceDir = "${clang-unwrapped.lib}/lib/clang/${clangVersion}/include";
  cflagsSgx = builtins.concatStringsSep " " [
    "-isystem ${clangResourceDir}"
    "-isystem ${sgx-libc-shim}/include"
  ];
in
  makeSetupHook {
    name = "sgxCrossEnvBuildHook";
  } (
    writeShellScript "sgxCrossEnvBuildHook.sh" ''
      sgxCrossEnvBuildHook() {
        # Use llvm toolchain for sgx since it's significantly better for
        # cross-compiling.

        # CC and CFLAGS are for `cc` build.rs helper crate
        export CC_x86_64_fortanix_unknown_sgx="${clang-unwrapped}/bin/clang"
        export CFLAGS_x86_64_fortanix_unknown_sgx="${cflagsSgx}"
        # CARGO_TARGET is for `cargo`
        export CARGO_TARGET_X86_64_FORTANIX_UNKNOWN_SGX_LINKER="${lld}/bin/ld.lld"
      }

      preBuildHooks+=(sgxCrossEnvBuildHook)
    ''
  )
