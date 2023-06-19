# Overlay to get a `lexeRustToolchain` attribute with the Lexe Rust toolchain.
# Provides cargo, clippy, cargo-fmt, rustdoc, rustfmt, and other tools.
#
# Basic usage:
#
# ```nix
# overlays = [ lexe-rust-overlay.overlays.default ];
# ```
#
# Use in combination with other overlays before or after:
#
# ```nix
# overlays = [
#   (import base-overlay)
#   lexe-rust-overlay.overlays.default
#   higher-level-overlay.overlays.default
# ];
# ```
#
# (Advanced) Override default behavior
# 
# ```nix
# overlays = [
#   (import base-overlay)
#   lexe-rust-overlay.overlays.oxalica-rust-overlay
#   (self: super: {
#     lexeRustToolchain = let
#       rustToolchainConfig = builtins.fromTOML (
#         builtins.readFile ../../public/rust-toolchain.toml
#       );
#       nightlyDate = builtins.replaceStrings [ "nightly-" ] [ "" ] rustToolchainConfig.toolchain.channel;
#     in
#     super.rust-bin.nightly.${nightlyDate}.default.override {
#       targets = rustToolchainConfig.toolchain.targets;
#     };
#   })
# ];
# ```
{
  description = "Lexe Rust toolchain overlay";

  inputs = {
    # `follows` indicates that the input must be passed in by the parent.
    # Failure to do so results in a segfault.
    nixpkgs.follows = "nixpkgs";
    rust-toolchain-toml.follows = "rust-toolchain-toml";

    # A nixpkgs overlay which provides pure and reproducible Rust toolchains.
    # Our overlay basically just configures oxalica/rust-overlay to use the
    # nightly version and targets specified in our rust-toolchain.toml.
    oxalica-rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-toolchain-toml, oxalica-rust-overlay }:
    let
      # Read Rust toolchain config from rust-toolchain.toml
      rustToolchainConfig =
        builtins.fromTOML (builtins.readFile rust-toolchain-toml);

      # Remove 'nightly-' from the channel string to get the nightly date
      nightlyDate = builtins.replaceStrings
        [ "nightly-" ] [ "" ] rustToolchainConfig.toolchain.channel;

      oxalicaRustOverlay =
        assert builtins.isFunction oxalica-rust-overlay.overlays.default;
        oxalica-rust-overlay.overlays.default;

      lexeRustOverlay = self: super:
        let
          # Apply oxalicaRustOverlay to super
          pkgsWithOxalica = super.extend oxalicaRustOverlay;
        in
        {
          lexeRustToolchain =
            assert builtins.isList rustToolchainConfig.toolchain.targets;
            pkgsWithOxalica.rust-bin.nightly.${nightlyDate}.default.override {
              targets = rustToolchainConfig.toolchain.targets;
            };
        };
    in
    {
      # The idiomatic way to output overlays in nix flakes.
      overlays = {
        # The primary output of this flake.
        default = lexeRustOverlay;
        # Expose the underlying oxalica/rust-overlay for more advanced usage.
        inherit oxalica-rust-overlay;
      };
    };
}
