# A rustc custom target-spec.json for x86_64-fortanix-unknown-sgx with LVI-CFI
# and LVI-LOAD mitigations removed.
#
# Regen: `just rust-regen-sgx-target-spec`

{
  jq,
  rustLexeToolchain,
  runCommand,
}:

runCommand "x86_64-fortanix-unknown-sgx-nolvi-json"
  {
    nativeBuildInputs = [
      jq
      rustLexeToolchain
    ];
  }

  # - Add default-uwtable flag so we get backtraces with -Zbuild-std
  # - Build SGX enclaves to support CPUs 2015+ with standard crypto intrinsics.
  # - Remove +lvi-cfi and +lvi-load-hardening rustc codegen flags
  # - Remove LLVM LVI flag

  ''
    mkdir -p $out

    # Dump stock x86_64-fortanix-unknown-sgx target spec JSON
    RUSTC_BOOTSTRAP=1 rustc -Z unstable-options \
      --target=x86_64-fortanix-unknown-sgx --print target-spec-json \
      | jq . \
      > $out/x86_64-fortanix-unknown-sgx-std.json

    # Modify target spec
    jq ' .
       | .metadata.description = (.metadata.description + " with unnecessary mitigations disabled")
       | .cpu = "x86-64-v3"
       | ."default-uwtable" = true
       | .features = (
           .features
           | split(",")
           - ["+lvi-cfi", "+lvi-load-hardening"]
           + ["+adx", "+aes", "+pclmul", "+sha", "+vaes", "+rdrnd", "+rdseed"]
           | unique
           | join(",")
         )
       | ."llvm-args" = (."llvm-args" - ["--x86-experimental-lvi-inline-asm-hardening"])
       ' \
      < $out/x86_64-fortanix-unknown-sgx-std.json \
      > $out/x86_64-fortanix-unknown-sgx.json
  ''
