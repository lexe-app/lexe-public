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

  # Parse the git revision of a git dependency from a `Cargo.lock` file, using
  # the github URL (e.g., "https://github.com/lexe-app/rust-sgx") as the key.
  #
  # Returns: an attrset you can pass directly to `builtins.fetchGit`
  #
  # ```
  # > parseCargoLockGitDep {
  # >   cargoLock = ../../Cargo.lock;
  # >   githubUrl = "https://github.com/lexe-app/rust-sgx";
  # > }
  # {
  #   url = "https://github.com/lexe-app/rust-sgx.git";
  #   ref = "lexe-2023_09_27";
  #   rev = "4aa8f13487c772dd4d24b7cc54bd2d5432803f7a";
  # }
  # ```
  parseCargoLockGitDep = {
    # The path to the `Cargo.lock` file.
    cargoLock ? throw "Requires `cargoLock` or `cargoLockContents` arg",
    cargoLockContents ? builtins.readFile cargoLock,
    githubUrl,
  }: let
    inherit (builtins) elemAt filter head match;
    inherit (lib) splitString escapeRegex;

    lines = splitString "\n" cargoLockContents;
    escapedUrl = escapeRegex githubUrl;
    pattern = "source = \"git\\+${escapedUrl}\\?branch=([^#]+)#([0-9a-f]{40})\"";
    firstMatchingLine = head (filter (line: (match pattern line) != null) lines);
    matches = match pattern firstMatchingLine;
  in {
    url = "${githubUrl}.git";
    ref = elemAt matches 0;
    rev = elemAt matches 1;
  };

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
}
