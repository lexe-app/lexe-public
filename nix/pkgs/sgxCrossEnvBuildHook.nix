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
}:
let
  # Use the base, "unwrapped" clang toolchain without all the nix
  # cross-compiling magic.
  #
  # 1. Less magic moving parts makes it easier to see what's happening.
  # 2. We don't yet have a proper nix system definition for Fortanix SGX (and
  #    probably won't, because it doesn't have a proper libc).
  # 3. Targeting Fortanix SGX is like targeting embedded (no-libc), which nix's
  #    "wrapped" toolchains don't handle very well.
  lld = llvmPackages.lld;
  clang-unwrapped = llvmPackages.clang-unwrapped;
  clangVersion = lib.versions.major clang-unwrapped.version;
  clangResourceDir = "${clang-unwrapped.lib}/lib/clang/${clangVersion}/include";
  cflagsSgx = builtins.concatStringsSep " " [
    "-isystem ${clangResourceDir}"
    "-isystem ${sgx-libc-shim}/include"
  ];
in
makeSetupHook
  {
    name = "sgxCrossEnvBuildHook";
  }
  (
    writeShellScript "sgxCrossEnvBuildHook.sh" ''
      sgxCrossEnvBuildHook() {
        # Use llvm toolchain for sgx since it's significantly better for
        # cross-compiling.

        # `TARGET_CC` and `TARGET_CFLAGS` are for the `cc` build.rs helper crate.
        # See: <https://docs.rs/cc/latest/cc/#external-configuration-via-environment-variables>
        export CC_x86_64_fortanix_unknown_sgx="${clang-unwrapped}/bin/clang"
        export CFLAGS_x86_64_fortanix_unknown_sgx="${cflagsSgx}"
        # `CARGO_TARGET` is for `cargo`.
        # See: <https://doc.rust-lang.org/cargo/reference/environment-variables.html#configuration-environment-variables>
        export CARGO_TARGET_X86_64_FORTANIX_UNKNOWN_SGX_LINKER="${lld}/bin/ld.lld"
        # `RUSTC_BOOTSTRAP=1` allows us to enable the one nightly feature we
        # need (sgx_platform) for a few SGX-patched crates, even with a stable
        # compiler.
        export RUSTC_BOOTSTRAP=1
      }

      preBuildHooks+=(sgxCrossEnvBuildHook)
    ''
  )
