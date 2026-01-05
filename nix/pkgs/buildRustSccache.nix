# A wrapper around `craneLib.buildPackage` but can use `sccache` to improve
# build times.
#
# `sccache` requires a persistent cache directory, which means giving the nix
# build sandbox access to this shared, global directory, `/var/cache/lexe`.
#
#
# ## Host cache directory setup:
#
# Follow the steps in <../../public/README.md#fast-incremental-cargo-builds-in-nix>.
#
#
# ## Why
#
# The original `rustBuildIncremental` actually produces bad output in some
# cases.
#
# (1) it purely relies on `cargo` incremental build correctness.
# (2) nix flakes copies the repo source to the /nix/store.
# (3) for reproducibility, everything in /nix/store has mtime == 1970-01-01.
# (4) cargo's rebuild checking only rebuilds if file.mtime > cache.mtime,
#     so we get false negatives (cargo believes the file is unchanged when
#     it is actually different).
#
# Relevant issue:
# [cargo - fingerprint by hash instead of mtime](https://github.com/rust-lang/cargo/issues/6529)
#
#
# ## Introducing sccache
#
# `sccache` caches individual `rustc` invocations and stores the results
# in `$SCCACHE_DIR = /var/cache/lexe/sccache`. Using `sccache`:
#
# - cached builds are about 0.4-0.5x time (faster)
# - uncached builds are about 1.1-1.2x time (extra overhead)
# - sccache sadly can't cache proc-macro crates...
# - sccache is somewhat less effective on macOS, since macOS doesn't support
#   proper chroot+bind mounts, which means nix has to run the build in a
#   directory like `/private/tmp/nix-build-secretctl-deps-0.1.0.drv-0/build`
#   instead of just `/build` like linux. This means caching will only happen
#   per-derivation rather than across derivations. Running multiple concurrent
#   builds will also not share caches.
#
# ## Shared build directory Explanation
#
# Here we setup a persistent, shared build directory, `/var/cache/lexe` with
# some special permissions. The main idea here is that we want all users in the
# `nixbld` group to be able to effortlessly manipulate files in the shared
# directory without locking any files to a specific user.
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
  # nixpkgs inputs
  #
  lib,
  sccache,
  #
  # Lexe inputs
  #
  cargoVendorDir,
  craneLib,
  lexePubLib,
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
  # Set this to `false` to unconditionally skip the shared build cache and just
  # build from scratch every time.
  enableSccache ? true,
  # If `true`, skip the separate dependencies-only build derivation. The extra
  # step is not super useful for crates outside our workspace.
  skipDepsOnlyBuild ? false,
  # If `true` (and we're building for x86), then enable x86-64-v3 and various
  # other CPU intrinsics. Enable if this touches any key material.
  # See .cargo/config.toml for more info.
  buildForLexeInfra ? true,
  ...
}@args:
#
let
  crateInfo = (builtins.fromTOML (builtins.readFile cargoToml)).package;
  crateVersion =
    if (crateInfo.version.workspace or false) then
      workspaceVersion
    else
      crateInfo.version;

  pname = args.pname or crateInfo.name;

  cleanedArgs = builtins.removeAttrs args [
    "cargoToml"
    "enableSccache"
    "skipDepsOnlyBuild"
  ];

  commonPackageArgs = cleanedArgs // {
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

    env.ENABLE_SCCACHE = enableSccache;

    # sccache runs an ephemeral localhost server while the derivation is
    # building. We need this setting on macOS to allow the server to bind.
    __darwinAllowLocalNetworking = enableSccache;

    env.CARGO_INCREMENTAL = "false";

    configurePhase = ''
      runHook preConfigure

      tryUseSccache() {
        # Check if the shared build cache dir is available.
        if [[ ! -d /var/cache/lexe ]]; then
          echo "WARN: shared lexe build cache directory (/var/cache/lexe) does not"
          echo "      exist or is not exposed to the nix build sandbox!"
          echo "      This means we will have to build from scratch without sccache."
          return
        fi
        if [[ ! -w /var/cache/lexe ]]; then
          echo "WARN: we don't have permissions for the shared lexe build cache "
          echo "      directory (/var/cache/lexe)!"
          echo "      This means we will have to build from scratch without sccache."
          return
        fi

        if [[ -d /var/cache/lexe && -w /var/cache/lexe ]]; then
          export SCCACHE_ENABLED=1
          export SCCACHE_DIR="/var/cache/lexe/sccache"
          export SCCACHE_LOG="warn"
          export RUSTC_WRAPPER="${sccache}/bin/sccache";

          ${sccache}/bin/sccache --show-stats

          # Without this, nix will complain when it statically links stuff from the
          # persistent cache dir. Not actually a problem here.
          unset NIX_ENFORCE_PURITY

          echo "Successfully setup shared cargo build cache"
        fi
      }

      if [[ $ENABLE_SCCACHE ]]; then
        echo "Trying to enable shared cargo build cache"
        tryUseSccache
      else
        echo "Skipping shared cargo build cache setup"
      fi
    ''
    + lib.optionalString buildForLexeInfra ''
      export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-Ctarget-cpu=x86-64-v3 -Ctarget-feature=+adx,+aes,+pclmulqdq,+sha"
    ''
    + ''
      runHook postConfigure
    '';

    cargoExtraArgs = args.cargoExtraArgs or "--package=${pname} --locked --offline";

    postBuild = ''
      # Print out the sccache cache hit/miss stats after building
      if [[ $SCCACHE_ENABLED ]]; then
        ${sccache}/bin/sccache --show-stats
      fi
    '';
  };

  # Compile external dependencies in a separate derivation.
  #
  # For workspace crates, this means we can often skip recompiling dependencies
  # if only workspace code has changed.
  depsOnly = craneLib.buildDepsOnly (
    builtins.removeAttrs commonPackageArgs [
      # For depsOnly build, I don't think we want any custom install/fixup phase
      # work, since it's not actually building the real output binary/library.
      # So let's filter out these args before passing down to crane.
      "preInstall"
      "installPhase"
      "postInstall"

      "preFixup"
      "fixupPhase"
      "postFixup"
    ]
    // {
      # HACK: The fake package Cargo.toml doesn't contain bin sections, so we must
      # compile just the --package
      # TODO(phlip9): remove this when crane is smart enough
      cargoExtraArgs =
        lexePubLib.regexReplaceAll "--bin( |=)[^ ]+" ""
          commonPackageArgs.cargoExtraArgs;
    }
  );
in
craneLib.buildPackage (
  commonPackageArgs
  // lib.optionalAttrs (!skipDepsOnlyBuild) {
    cargoArtifacts = depsOnly;
  }
)
