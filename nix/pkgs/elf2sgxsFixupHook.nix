# Add this hook to a package's `nativeBuildInputs` to convert the ELF binary
# into a `.sgxs` enclave binary, during the `postFixup` phase.
{
  #
  # nixpkgs
  #
  lib,
  makeSetupHook,
  writeShellScript,
  #
  # lexePkgs
  #
  ftxsgx-elf2sgxs,
}:
# We use the settings in the Cargo.toml `[package.metadata.fortanix-sgx]`
# section to configure the enclave threads, stack size, etc...
{
  cargoToml ? throw "either cargoToml or cargoTomlContents must be specified",
  cargoTomlContents ? builtins.readFile cargoToml,
  cargoTomlParsed ? builtins.fromTOML cargoTomlContents,
  # By default, infer the bin name from the name of the package `Cargo.toml`
  binName ? cargoTomlParsed.package.name,
}:
#
let
  inherit (builtins) mapAttrs toString;

  # convert all values in an attrset to strings
  valuesToString = attrs: (mapAttrs (_name: toString) attrs);

  settings = valuesToString cargoTomlParsed.package.metadata.fortanix-sgx;
  debugFlag =
    # NOTE: nix coerces `true` -> `"1"`
    if (settings.debug == "1") then "--debug" else "";
in
makeSetupHook
  {
    name = "elf2sgxsFixupHook";

    # Add to the $PATH
    propagatedBuildInputs = [ ftxsgx-elf2sgxs ];
  }
  (
    writeShellScript "elf2sgxsFixupHook.sh" ''
      elf2sgxsFixupHook() {
        local binPath="$out/bin/${binName}"
        local sgxsPath="$binPath.sgxs"

        # build the `<binName>.sgxs` enclave binary
        ftxsgx-elf2sgxs $binPath --output $sgxsPath \
          --heap-size ${settings.heap-size} \
          --ssaframesize ${settings.ssaframesize} \
          --stack-size ${settings.stack-size} \
          --threads ${settings.threads} \
          ${debugFlag}

        # compute the enclave measurement (SHA-256 hash of the enclave binary)
        # and dump it into `<binName>.measurement`
        local measurement=$(sha256sum --binary $sgxsPath | cut -d ' ' -f 1)
        echo -n "$measurement" > $binPath.measurement
        echo "SGXS enclave measurement: \"$measurement\""
        echo "SGXS enclave size: $(stat --format='%s' $sgxsPath)"
      }

      postFixupHooks+=(elf2sgxsFixupHook)
    ''
  )
