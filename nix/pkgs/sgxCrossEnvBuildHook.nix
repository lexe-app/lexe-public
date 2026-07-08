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
  sgxLlvmLibunwind,
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
  bintools-unwrapped = llvmPackages.bintools-unwrapped;
  lld = llvmPackages.lld;
  clang-unwrapped = llvmPackages.clang-unwrapped;
  clangVersion = lib.versions.major clang-unwrapped.version;
  clangResourceDir = "${clang-unwrapped.lib}/lib/clang/${clangVersion}";
  cflagsSgx = builtins.concatStringsSep " " [
    # See: SGX options in <https://github.com/rust-lang/rust/blob/main/src/ci/docker/host-x86_64/dist-various-2/Dockerfile>
    "-D__ELF__"
    "-DCMAKE_SYSTEM_NAME=Generic-ELF"

    # Headers for clang compiler builtins/intrinsics
    "-resource-dir ${clangResourceDir}"

    # Headers for the few libc fns/macros our native deps require
    "-idirafter ${sgx-libc-shim}/include"
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

        # These `<var>_<target>` envs are for the `cc` build.rs helper crate.
        # See: <https://docs.rs/cc/latest/cc/#external-configuration-via-environment-variables>
        export AR_x86_64_fortanix_unknown_sgx="${bintools-unwrapped}/bin/ar"
        export CC_x86_64_fortanix_unknown_sgx="${clang-unwrapped}/bin/clang"
        export CFLAGS_x86_64_fortanix_unknown_sgx="${cflagsSgx}"
        # Complain loudly if we try to compile C++ code
        export CXX_x86_64_fortanix_unknown_sgx="false"
        # `CARGO_TARGET_<target>_<var>` is for `cargo`.
        # See: <https://doc.rust-lang.org/cargo/reference/environment-variables.html#configuration-environment-variables>
        export CARGO_TARGET_X86_64_FORTANIX_UNKNOWN_SGX_LINKER="${lld}/bin/ld.lld"
        export CARGO_TARGET_X86_64_FORTANIX_UNKNOWN_SGX_RUSTFLAGS="-Lnative=${sgxLlvmLibunwind}/lib"
        # `RUSTC_BOOTSTRAP=1` allows us to enable the one nightly feature we
        # need (sgx_platform) for a few SGX-patched crates, even with a stable
        # compiler.
        export RUSTC_BOOTSTRAP=1
      }

      preBuildHooks+=(sgxCrossEnvBuildHook)
    ''
  )
