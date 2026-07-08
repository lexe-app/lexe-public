# LLVM libunwind built for x86_64-fortanix-unknown-sgx from our pinned rust-src.
#
# Keep this in sync with Rust bootstrap's `Libunwind::run` build in:
# `rust/src/bootstrap/src/core/build_steps/llvm.rs`.
{
  lib,
  llvmPackages,
  runCommand,
  rustLexeToolchain,
  sgx-libc-shim,
}:
let
  bintools-unwrapped = llvmPackages.bintools-unwrapped;
  clang-unwrapped = llvmPackages.clang-unwrapped;
  clangVersion = lib.versions.major clang-unwrapped.version;
  clangResourceDir = "${clang-unwrapped.lib}/lib/clang/${clangVersion}";
  libunwindSrc = "${rustLexeToolchain}/lib/rustlib/src/rust/src/llvm-project/libunwind";

  commonFlags = [
    # Rust bootstrap asks cc::Build to target x86_64-fortanix-unknown-sgx, but
    # our Clang rejects that triple. x86_64-elf supplies the ELF/x86_64 base;
    # RUST_SGX and the SGX libunwind flags below select the SGX behavior.
    "--target=x86_64-elf"
    "-U_FORTIFY_SOURCE"
    "-D_FORTIFY_SOURCE=0"
    "-D_LIBUNWIND_DISABLE_VISIBILITY_ANNOTATIONS"
    "-D_LIBUNWIND_IS_BAREMETAL"
    "-D_LIBUNWIND_IS_NATIVE_ONLY=1"
    "-D__NO_MATH_INLINES"
    "-D__NO_STRING_INLINES"
    "-DNDEBUG"
    "-DRUST_SGX=1"
    "-O3"
    "-fexceptions"
    # Bootstrap does not pass -fPIC explicitly. Upstream libunwind CMake does,
    # and our direct Clang invocation should not depend on cc/CMake defaults.
    "-fPIC"
    "-ffreestanding"
    "-fno-stack-protector"
    "-fstrict-aliasing"
    "-funwind-tables"
    "-fvisibility=hidden"
    "-I${libunwindSrc}/include"

    # Headers for clang compiler builtins/intrinsics. We need to provide this
    # since we're using clang-unwrapped.
    "-resource-dir"
    clangResourceDir

    # Bootstrap gets target libc/compiler headers from an
    # x86_64-unknown-linux-gnu sysroot, but we use a minimal SGX libc shim.
    "-idirafter"
    "${sgx-libc-shim}/include"
  ];

  cFlags = lib.escapeShellArgs (commonFlags ++ [ "-std=c99" ]);
  cxxFlags = lib.escapeShellArgs (
    commonFlags
    ++ [
      # Bootstrap adds C++ -fno-exceptions before the common SGX -fexceptions;
      # the latter wins. Keep only the effective flag.
      "-fno-rtti"
      # Bootstrap probes the deprecated -fvisibility-global-new-delete-hidden
      # spelling.
      "-fvisibility-global-new-delete=force-hidden"
      "-nostdinc++"
      "-std=c++11"
    ]
  );

  cSources = [
    "Unwind-sjlj.c"
    "UnwindLevel1-gcc-ext.c"
    "UnwindLevel1.c"
    "UnwindRegistersRestore.S"
    "UnwindRegistersSave.S"
    "UnwindRustSgx.c"
  ];
  cxxSources = [
    "Unwind-EHABI.cpp"
    "Unwind-seh.cpp"
    "libunwind.cpp"
  ];
in
runCommand "sgx-llvm-libunwind"
  {
    nativeBuildInputs = [
      bintools-unwrapped
      clang-unwrapped
    ];
  }
  ''
    mkdir -p "$out/lib" "$TMPDIR/include" "$TMPDIR/obj"
    # Bootstrap relies on the target compiler/sysroot headers. This direct build
    # provides only the libunwind includes, so keep the small headers libunwind
    # actually includes while compiling this SGX source set.
    cat > "$TMPDIR/include/inttypes.h" <<'EOF'
    #ifndef _INTTYPES_H
    #define _INTTYPES_H 1
    #include <stdint.h>
    #define PRId64 "ld"
    #define PRIdPTR "ld"
    #define PRIu8 "u"
    #define PRIu64 "lu"
    #define PRIuPTR "lu"
    #define PRIx64 "lx"
    #define PRIxPTR "lx"
    #endif
    EOF
    touch "$TMPDIR/include/elf.h" "$TMPDIR/include/link.h"

    objects=()

    # Bootstrap drives these compiles through cc::Build and lets the C build
    # absorb the C++ objects into libunwind.a. This standalone derivation uses
    # explicit Clang invocations and archives the same object set with llvm-ar.
    for src in ${lib.escapeShellArgs cSources}; do
      obj="$TMPDIR/obj/$src.o"
      clang -I "$TMPDIR/include" ${cFlags} -c "${libunwindSrc}/src/$src" -o "$obj"
      objects+=("$obj")
    done

    for src in ${lib.escapeShellArgs cxxSources}; do
      obj="$TMPDIR/obj/$src.o"
      clang++ -I "$TMPDIR/include" ${cxxFlags} -c "${libunwindSrc}/src/$src" -o "$obj"
      objects+=("$obj")
    done

    llvm-ar crs "$out/lib/libunwind.a" "''${objects[@]}"
  ''
