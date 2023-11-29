# # Build Rust packages fast w/ less purity
#
# ## cargo target-dir sandbox hole-punch:
#
# We'll be giving the nix build sandbox access to a shared, global cargo
# `target/` directory so we can fully reuse intermediate cargo build artifacts
# across `nix build` invocations.
#
# Pros:
# 1. fast incremental builds (change one line -> only few seconds rebuild)
# 2. still retain almost all sandboxing (no network access, all other files sandboxed)
#
# Cons:
# 1. probably not suited for true reproducible builds, since incremental builds
#    are unlikely to be reproducible (my guess, though haven't tested)
# 2. not "true" purity
# 3. extra setup steps for new dev
# 4. user has to remember to delete this /var/cache/lexe/target folder every
#    once in a while (when their disk fills up LOL)
# 5. `cargo clean` won't clean this dir for us :'(
#
# ## Host setup:
#
# Follow the steps in <../../public/README.md#fast-incremental-cargo-builds-in-nix>.
#
# ## Explanation
#
# Here we setup a persistent, shared build directory with some special
# permissions. The main idea here is that we want all users in the `nixbld`
# group to be able to effortlessly manipulate files in the shared directory
# without locking any files to a specific user.
#
# For security, we only allow `nixbld` users to access the shared directory
# (though truthfully, anyone with `nix daemon` access could probably figure out
# how to poison the shared build cache...)
#
# We specify '2' in the 2770 modifier to enable the SETGID bit. SETGID on a
# directory like this will _propagate_ the `nixbld` group to all newly created
# files/dirs in the shared build dir.
#
# We also set a default file ACL which makes all newly created files/dirs have
# group r/w/x permissions. That way, even if we run multiple concurrent
# `nix build` commands, they will correctly serialize access to the shared dir
# (cargo has its build locking system) and produce files that are
# read/write/exec-able by all other `nixbld` users.
{
  #
  # Lexe inputs
  #
  craneLib,
  cargoVendorDir,
  srcRust,
  workspaceVersion,
}:
#
{
  #
  # options
  #
  # Path to crate Cargo.toml
  cargoToml,
  # Set this to `false` to unconditionally skip the shared cargo build cache and
  # just build from scratch every time.
  # TODO(phlip9): need to fix cache... probably use sccache?
  enableSharedCargoBuildCache ? true,
  ...
} @ args:
#
let
  crateInfo = (builtins.fromTOML (builtins.readFile cargoToml)).package;
  crateVersion =
    if (crateInfo.version.workspace or false)
    then workspaceVersion
    else crateInfo.version;

  pname = args.pname or crateInfo.name;

  cleanedArgs = builtins.removeAttrs args ["cargoToml" "enableSharedCargoBuildCache"];
in
  craneLib.buildPackage (cleanedArgs
    // {
      pname = pname;
      version = args.version or crateVersion;

      src = args.src or srcRust;

      # Ensure strict separation between build-time and runtime deps.
      # Ex: don't allow dynamically linking against .so's in a build-time dep.
      strictDeps = args.strictDeps or true;

      # A directory of vendored cargo sources which can be consumed without
      # network access. Directory structure should basically follow the output
      # of `cargo vendor`.
      cargoVendorDir = args.cargoVendorDir or cargoVendorDir;

      # Don't install target/ dir to /nix/store. We have the sandbox hole-punch
      # for this.
      doInstallCargoArtifacts = false;
      cargoArtifacts = null;

      enableSharedCargoBuildCache = enableSharedCargoBuildCache;

      configurePhase = ''
        runHook preConfigure

        tryUseSharedCargoBuildCache() {
          # Check if the shared build cache is available.
          if [[ ! -d /var/cache/lexe ]]; then
            echo "WARN: shared lexe build cache directory (/var/cache/lexe) does not"
            echo "      exist or is not exposed to the nix build sandbox!"
            echo "      This means we will have to build from scratch without a cache."
            return
          fi
          if [[ ! -w /var/cache/lexe ]]; then
            echo "WARN: we don't have permissions for the shared lexe build cache "
            echo "      directory (/var/cache/lexe)!"
            echo "      This means we will have to build from scratch without a cache."
            return
          fi

          if [[ -d /var/cache/lexe && -w /var/cache/lexe ]]; then
            export CARGO_TARGET_DIR=/var/cache/lexe/target

            # Without this, nix will complain when it statically links stuff from the
            # persistent cache dir. Not actually a problem here.
            unset NIX_ENFORCE_PURITY

            echo "Successfully setup shared cargo build cache"
          fi
        }

        if [[ $enableSharedCargoBuildCache ]]; then
          echo "Trying to enable shared cargo build cache"
          tryUseSharedCargoBuildCache
        else
          echo "Skipping shared cargo build cache setup"
        fi

        runHook postConfigure
      '';

      cargoExtraArgs = args.cargoExtraArgs or "--package ${pname} --locked --offline";
    })
