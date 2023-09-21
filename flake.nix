{
  description = "Lexe public flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    # pure, reproducible rust toolchain overlay
    #
    # we must use a nightly rust toolchain for SGX reasons, so we can't use the
    # rust toolchain from nixpkgs.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs"; # use our nixpkgs ver
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
  }: let
    # supported systems
    systems = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
      "x86_64-darwin"
    ];

    # genAttrs :: [ String ] -> (String -> Any) -> AttrSet
    #
    # ```
    # > genAttrs [ "bob" "joe" ] (name: "hello ${name}")
    # { bob = "hello bob"; joe = "hello joe" }
    # ```
    genAttrs = nixpkgs.lib.genAttrs;

    # eachSystem :: (String -> AttrSet) -> AttrSet
    #
    # ```
    # > eachSystem (system: { a = 123; b = "cool ${system}"; })
    # {
    #   "aarch64-darwin" = {
    #     a = 123;
    #     b = "cool aarch64-darwin";
    #   };
    #   "x86_64-linux" = {
    #     a = 123;
    #     b = "cool x86_64-linux";
    #   };
    # }
    # ```
    eachSystem = builder: genAttrs systems builder;

    # The "host" nixpkgs for each system.
    #
    # ```
    # {
    #   "aarch64-darwin" = <nixpkgs>;
    #   "x86_64-linux" = <nixpkgs>;
    # }
    # ```
    systemPkgs = eachSystem (system:
      import nixpkgs {
        system = system;
        overlays = [
          # adds `rust-bin.fromRustupToolchainFile` to this pkgs instance.
          rust-overlay.overlays.default

          # adds `rust-lexe` with our configured toolchain settings from
          # `./rust-toolchain.toml`
          (self: super: {
            rust-lexe = super.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          })
        ];
      });

    # eachSystemPkgs :: (Nixpkgs -> AttrSet) -> AttrSet
    eachSystemPkgs = builder:
      eachSystem (
        system:
          builder systemPkgs.${system}
      );
  in {
    # The *.nix file formatter.
    formatter = eachSystemPkgs (pkgs: pkgs.alejandra);

    pkgs = systemPkgs;

    # devShells = eachSystemPkgs (pkgs: {
    #   default = pkgs.mkShellNoCC {
    #     packages = [pkgs.rust-lexe];
    #   };
    # });
  };
}
