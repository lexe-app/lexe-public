{
  description = "Lexe public monorepo flake";

  inputs = {
    # NixOS/nixpkgs - nixos-stable branch for the current release
    # nixpkgs.url = "github:nixos/nixpkgs/release-25.05";
    nixpkgs.url = "github:phlip9/nixpkgs/release-25.05-sgx-psw-v2.26";
    # nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

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
      inputs = {
        nixpkgs.follows = "nixpkgs";
        rust-analyzer-src.follows = "";
      };
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

    # Host nixpkgs set that allows "unfree" packages, like the Android SDK.
    # Only used for building the Android app.
    systemPkgsUnfree = eachSystem (system: lexePubLib.mkPkgsUnfree inputs.nixpkgs system);

    # eachSystemPkgs :: (builder :: Nixpkgs -> AttrSet) -> AttrSet
    eachSystemPkgs = builder: eachSystem (system: builder systemPkgs.${system});

    # All lexe public monorepo packages and package helpers, for each host
    # system.
    systemLexePubPkgs = eachSystem (system:
      import ./nix/pkgs/default.nix {
        lib = inputs.nixpkgs.lib;
        pkgs = systemPkgs.${system};
        pkgsUnfree = systemPkgsUnfree.${system};
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
    devShells = eachSystem (system: let
      lib = inputs.nixpkgs.lib;
      pkgs = systemPkgs.${system};
      lexePubPkgs = systemLexePubPkgs.${system};
      lexePubDevShells = import ./nix/devShells {
        lib = lib;
        pkgs = pkgs;
        lexePubPkgs = lexePubPkgs;
      };
    in rec {
      # The default dev shell for `nix develop`.
      default = sgx;

      # compile Rust SGX enclaves
      sgx = lexePubDevShells.sgx;

      #
      # app
      #

      # app flutter_rust_bridge codegen
      app-rs-codegen = lexePubDevShells.app-rs-codegen;

      # Android app development toolchains
      app-android = lexePubDevShells.app-android;

      # iOS/macOS app development toolchains
      app-ios-macos = lexePubDevShells.app-ios-macos;
    });

    # The *.nix file formatter.
    # Run with `nix fmt`.
    formatter = eachSystemPkgs (pkgs: pkgs.alejandra);
  };
}
