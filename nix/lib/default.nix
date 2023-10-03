# `lexeLib`
#
# nix-only library code that doesn't depend on packages.
{lib}: rec {
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
}
