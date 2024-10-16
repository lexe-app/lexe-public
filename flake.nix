{
  description = "Lexe public monorepo flake";

  inputs = {
    # NixOS/nixpkgs - nixos-stable branch for the current release
    # nixpkgs.url = "github:nixos/nixpkgs/nixos-24.05";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    # library for building rust projects. supports basic incremental cargo
    # artifact caching.
    crane.url = "github:ipetkov/crane";

    # Provides official pre-built rust toolchains.
    #
    # This flake is lighter weight than oxalica/rust-overlay since
    # 1. it doesn't use an overlay (costly during nix eval)
    # 2. it has a much smaller git repo (only tracks the latest stable/beta/nightly)
    #
    # TODO(phlip9): Use this until I can figure out how to get sgx cross build via nixpkgs.
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };
  };

  outputs = {self, ...} @ inputs: let
    lib = inputs.nixpkgs.lib;
    lexePubLib = import ./nix/lib/default.nix {lib = lib;};
    eachSystem = lexePubLib.eachSystem;

    # The "host" nixpkgs set for each system.
    #
    # ```
    # {
    #   "aarch64-darwin" = <pkgs>;
    #   "x86_64-linux" = <pkgs>;
    # }
    # ```
    systemPkgs = inputs.nixpkgs.legacyPackages;

    # eachSystemPkgs :: (builder :: Nixpkgs -> AttrSet) -> AttrSet
    eachSystemPkgs = builder: eachSystem (system: builder systemPkgs.${system});

    # All lexe public monorepo packages and package helpers, for each host
    # system.
    systemLexePubPkgs = eachSystem (system:
      import ./nix/pkgs/default.nix {
        lib = inputs.nixpkgs.lib;
        pkgs = systemPkgs.${system};
        crane = inputs.crane;
        fenixPkgs = inputs.fenix.packages.${system};
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
            bitcoind
            blockstream-electrs
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
