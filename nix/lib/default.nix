# `lexeLib`
#
# nix-only library code that doesn't depend on packages.
{lib}: rec {
  # supported host systems
  systems = [
    "x86_64-linux"
    "aarch64-linux"
    "aarch64-darwin"
  ];

  # genAttrs :: [ String ] -> (String -> Any) -> AttrSet
  #
  # ```
  # > genAttrs [ "bob" "joe" ] (name: "hello ${name}")
  # { bob = "hello bob"; joe = "hello joe" }
  # ```
  genAttrs = lib.genAttrs;

  # eachSystem :: (builder :: String -> AttrSet) -> AttrSet
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

  # In a string `inputStr`, replace all matches of `regex` with `replacement`.
  #
  # ```
  # > inputStr = "--package=run-sgx --bin=run-sgx --locked --offline"
  # > regexReplaceAll "--bin( |=)[^ ]+" "" inputStr
  # "--package=run-sgx  --locked --offline"
  # ```
  regexReplaceAll = regex: replacement: inputStr: let
    inherit (builtins) concatStringsSep isString map split;

    # ex: inputStr = "foo bar baz", regex = "bar" => [ "foo " ["bar"] " baz" ]
    splits = split regex inputStr;

    matchesReplaced = map (s:
      if isString s
      then s
      else replacement)
    splits;
  in
    concatStringsSep "" matchesReplaced;

  # mkPkgsUnfree :: NixpkgsFlakeInput -> String -> NixpkgsPackageSet
  #
  # Builds a `pkgs` set that allows unfree packages, like the Android SDK.
  # Only used for building the Android app. We keep this as a separate package
  # set for eval efficiency.
  mkPkgsUnfree = nixpkgsFlake: system:
    import nixpkgsFlake {
      system = system;
      config = {
        android_sdk.accept_license = true;
        allowUnfreePredicate = pkg:
          builtins.elem (lib.getName pkg) [
            "android-sdk-tools"
            "android-sdk-cmdline-tools"
          ];
      };
    };
}
