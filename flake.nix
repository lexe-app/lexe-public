{
  description = "Lexe public monorepo flake";

  inputs = {
    # nixpkgs unstable
    #
    # We use unstable as `oxalica/rust-overlay` seems to require it.
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    # We don't actually use these, but some dependencies do. Let's try to use the
    # same versions.
    flake-utils.url = "github:numtide/flake-utils";
    flake-compat.url = "https://flakehub.com/f/edolstra/flake-compat/1.tar.gz";

    # pure, reproducible, rust toolchain overlay. used to get toolchain from
    # our workspace `rust-toolchain.toml`.
    #
    # we must use a nightly rust toolchain for SGX reasons, so we can't use the
    # rust toolchain from nixpkgs.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs"; # use our nixpkgs version
        flake-utils.follows = "flake-utils";
      };
    };

    # library for building rust projects. supports basic incremental cargo
    # artifact caching.
    crane = {
      url = "github:ipetkov/crane";
      inputs = {
        nixpkgs.follows = "nixpkgs"; # use our nixpkgs version
        rust-overlay.follows = "rust-overlay";
        flake-utils.follows = "flake-utils";
        flake-compat.follows = "flake-compat";
      };
    };
  };

  outputs = {self, ...} @ inputs: let
    lib = inputs.nixpkgs.lib;
    lexePubLib = import ./nix/lib/default.nix {lib = lib;};
    eachSystem = lexePubLib.eachSystem;

    # The "host" nixpkgs for each system.
    #
    # ```
    # {
    #   "aarch64-darwin" = <nixpkgs>;
    #   "x86_64-linux" = <nixpkgs>;
    # }
    # ```
    systemPkgs = eachSystem (system:
      import inputs.nixpkgs {
        system = system;
        overlays = [
          # adds: `rust-bin.fromRustupToolchainFile` to this pkgs instance.
          inputs.rust-overlay.overlays.default
        ];
      });

    # eachSystemPkgs :: (builder :: Nixpkgs -> AttrSet) -> AttrSet
    eachSystemPkgs = builder: eachSystem (system: builder systemPkgs.${system});

    # All lexe public monorepo packages and package helpers, for each host
    # system.
    systemLexePubPkgs = eachSystem (system:
      import ./nix/pkgs/default.nix {
        lib = inputs.nixpkgs.lib;
        pkgs = systemPkgs.${system};
        crane = inputs.crane;
        lexePubLib = lexePubLib;
      });
  in {
    # The exposed lexe public monorepo packages.
    # ex: `nix build .#node-release-sgx`
    # ex: `nix run .#ftxsgx-elf2sgxs -- ...`
    packages = eachSystem (
      system: let
        lexePubPkgs = systemLexePubPkgs.${system};
      in
        {
          inherit
            (lexePubPkgs)
            ftxsgx-elf2sgxs
            node-debug-nosgx
            node-debug-sgx
            node-release-nosgx
            node-release-sgx
            ;
        }
        // lib.optionalAttrs (system == "x86_64-linux") {
          inherit
            (lexePubPkgs)
            run-sgx
            run-sgx-test
            sgx-detect
            sgx-test
            ;
        }
    );

    # lexe development shells
    # ex: `nix develop`
    devShells = eachSystemPkgs (pkgs: let
      lib = inputs.nixpkgs.lib;
      lexePubPkgs = systemLexePubPkgs.${pkgs.system};
    in {
      # default development shell
      default = pkgs.mkShell {
        name = "lexe";
        inputsFrom = [lexePubPkgs.node-release-sgx];
        packages = lib.optionals pkgs.stdenv.isDarwin [
          pkgs.darwin.apple_sdk.frameworks.Security
        ];
      };
    });

    # The *.nix file formatter.
    # Run with `nix fmt`.
    formatter = eachSystemPkgs (pkgs: pkgs.alejandra);
  };
}
