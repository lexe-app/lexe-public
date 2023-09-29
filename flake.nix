{
  description = "Lexe public flake";

  inputs = {
    # nixpkgs unstable
    #
    # Use unstable as `oxalica/rust-overlay` seems to require it.
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    # We don't actually use this, but some dependencies do. Let's try to use the
    # same version.
    flake-utils.url = "github:numtide/flake-utils";

    # pure, reproducible rust toolchain overlay. used to get toolchain from
    # `rust-toolchain.toml`.
    #
    # we must use a nightly rust toolchain for SGX reasons, so we can't use the
    # rust toolchain from nixpkgs.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs"; # use our nixpkgs version
      inputs.flake-utils.follows = "flake-utils";
    };

    # library for building rust projects. supports basic incremental cargo
    # artifact caching.
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
    # supported host systems
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
          # adds: `rust-bin.fromRustupToolchainFile` to this pkgs instance.
          # From: `oxalica/rust-overlay`
          rust-overlay.overlays.default
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

    # The lexe public monorepo packages
    packages = eachSystemPkgs (
      pkgs: let
        lexePkgs = import ./nix/pkgs/default.nix {
          pkgs = pkgs;
          crane = crane;
        };
      in {
        inherit
          (lexePkgs)
          ftxsgx-elf2sgxs
          node-release-sgx
          node-debug-sgx
          node-release-nosgx
          node-debug-nosgx
          ;
      }
    );

    # easy access from `nix repl`
    # > :load-flake .
    systemPkgs = systemPkgs;
    # sgxCrossPkgs = sgxCrossPkgs;
    lib = nixpkgs.lib;

    # devShells = eachSystemPkgs (pkgs: {
    #   default = pkgs.mkShellNoCC {
    #     packages = [pkgs.rust-lexe];
    #   };
    # });

    devShells = eachSystemPkgs (pkgs: {
      default = pkgs.mkShellNoCC {
        packages = [pkgs.diffoscopeMinimal pkgs.nix-diff];
      };
    });
  };
}
