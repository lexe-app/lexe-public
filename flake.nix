{
  description = "Lexe public flake";

  inputs = {
    # nixpkgs unstable
    #
    # `oxalica/rust-overlay` seems to require unstable.
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    # We don't actually use this, but some dependencies do. Let's try to use the
    # same version.
    flake-utils.url = "github:numtide/flake-utils";

    # pure, reproducible rust toolchain overlay. get toolchain from
    # `rust-toolchain.toml`.
    #
    # we must use a nightly rust toolchain for SGX reasons, so we can't use the
    # rust toolchain from nixpkgs.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs"; # use our nixpkgs version
      inputs.flake-utils.follows = "flake-utils";
    };

    # library for building rust projects. supports incremental artifact caching.
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs"; # use our nixpkgs version
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.flake-utils.follows = "flake-utils";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    crane,
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

          # adds
          # - `rustLexeToolchain` with our configured toolchain settings from
          #   `./rust-toolchain.toml`
          # - `craneLib`
          (self: super: {
            rustLexeToolchain =
              super.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

            craneLib = (crane.mkLib super).overrideToolchain self.rustLexeToolchain;
          })
        ];
      });

    # TODO(phlip9): can I just use `pkgsCross` from `systemPkgs` rather than a
    # fresh nixpkgs instance?
    sgxCrossPkgs = eachSystem (
      system:
        import nixpkgs {
          crossSystem = "x86_64-linux";
          localSystem = system;

          overlays = [
            rust-overlay.overlays.default

            (self: super: {
              rustLexeToolchain =
                super.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

              craneLib = (crane.mkLib super).overrideToolchain self.rustLexeToolchain;
            })
          ];
        }
    );

    # eachSystemPkgs :: (Nixpkgs -> AttrSet) -> AttrSet
    eachSystemPkgs = builder:
      eachSystem (
        system:
          builder systemPkgs.${system}
      );
  in {
    # The *.nix file formatter.
    formatter = eachSystemPkgs (pkgs: pkgs.alejandra);

    packages = eachSystem (system: {
      # (aarch64-darwin cross)
      # $ sha256sum < /nix/store/0iapipss8qi72539l58a6dvjns8g7zw1-node-fake-x86_64-unknown-linux-gnu-0.1.0-sgx/bin/node-fake
      # fa2cd55e0e98861179f98293702ef43f017a871b6a2264540adcb38b154fd3e7  -
      # Size: 332600 B
      #
      # (x86_64-linux vm)
      # $ sha256sum < /nix/store/h79gd5yz40gfm5mzz4chhjfy1kks7m33-node-fake-0.1.0-sgx/bin/node-fake
      # 2a10f869367e63516b0dedd44151b84f8aeedabd8ef2e3d0da232a3381262c6f  -
      # Size: 332584
      #
      # $ diffoscope /nix/store/h79gd5yz40gfm5mzz4chhjfy1kks7m33-node-fake-0.1.0-sgx/bin/node-fake /nix/store/0iapipss8qi72539l58a6dvjns8g7zw1-node-fake-x86_64-unknown-linux-gnu-0.1.0-sgx/bin/node-fake
      # very big diff...
      node-fake-sgx = sgxCrossPkgs.${system}.callPackage ./nix/pkgs/node-fake.nix {sgx = true;};
      node-fake-nosgx = systemPkgs.${system}.callPackage ./nix/pkgs/node-fake.nix {sgx = false;};
    });

    # pkgs = systemPkgs;

    # devShells = eachSystemPkgs (pkgs: {
    #   default = pkgs.mkShellNoCC {
    #     packages = [pkgs.rust-lexe];
    #   };
    # });

    devShells = eachSystemPkgs (pkgs: {
      default = pkgs.mkShellNoCC {
        packages = [pkgs.diffoscopeMinimal];
      };
    });
  };
}
# time nix build -L \
#   --json \
#   --eval-store auto \
#   --store ssh-ng://linux-builder@orb \
#   .#packages.x86_64-linux.node-fake-sgx \
#   | jq .
# nix copy \
#   --no-check-sigs \
#   --from ssh-ng://linux-builder@orb \
#   /nix/store/h79gd5yz40gfm5mzz4chhjfy1kks7m33-node-fake-0.1.0-sgx

