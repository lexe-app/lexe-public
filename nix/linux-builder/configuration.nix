# This file describes the OrbStack `linux-builder` VM, used for building
# x86_64-linux packages on macOS machines at near-native speed.
# At the bottom are some Lexe customizations.
#
# See: <../../README.md#orbstack-linux-builder-setup> for how to update the VM
# with this config.
{
  modulesPath,
  pkgs,
  ...
}: {
  #
  # ORBSTACK DEFAULT SETTINGS
  #

  imports = [
    # Include the default lxd configuration.
    #
    # This is configuration from the NixOS modules repo.
    "${modulesPath}/virtualisation/lxc-container.nix"
    # Include the container-specific autogenerated configuration.
    #
    # These are files present in the VM's `/etc/nixos/` directory, provided by
    # the default OrbStack NixOS install.
    ./lxd.nix
    ./orbstack.nix
  ];

  # The global useDHCP flag is deprecated, therefore explicitly set to false here.
  # Per-interface useDHCP will be mandatory in the future, so this generated config
  # replicates the default behaviour.
  networking.useDHCP = false;
  networking.interfaces.eth0.useDHCP = true;

  # This value determines the NixOS release from which the default
  # settings for stateful data, like file locations and database versions
  # on your system were taken. It‘s perfectly fine and recommended to leave
  # this value at the release version of the first install of this system.
  # Before changing this value read the documentation for this option
  # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
  system.stateVersion = "21.05"; # Did you read the comment?

  # As this is intended as a stadalone image, undo some of the minimal profile stuff
  documentation.enable = true;
  documentation.nixos.enable = true;
  environment.noXlibs = false;

  #
  # CUSTOM LINUX-BUILDER CONFIG
  #

  # Add all members of the `wheel` group to nix trusted-users.
  nix.settings.trusted-users = ["@wheel"];

  # Enable git so `nix build` works in git repos.
  programs.git.enable = true;

  nix.extraOptions = ''
    # `nix-command`: enable `nix XXX` (without hyphen) commands
    # `flakes`: enable nix flakes
    experimental-features = nix-command flakes

    # Sign packages in the VM nix store
    secret-key-files = /etc/nix/store-signing-key

    # Add path to share e.g. cargo target/ dir contents across builds
    extra-sandbox-paths = /var/cache/lexe
  '';

  # collect garbage monthly
  # deletes unused items in the `/nix/store` to save some disk space.
  nix.gc = {
    automatic = true;
    dates = "monthly";
  };

  # Add some packages to the environment
  environment.systemPackages = [
    pkgs.htop
    # Use with `breakpointHook` to debug broken package builds. The hook will
    # pause the build if it breaks, letting you drop into a container to inspect.
    # See: <https://nixos.org/manual/nixpkgs/stable/#breakpointhook>
    pkgs.cntr
  ];

  # automatically generate a signing key for the VM's local nix store at startup
  # if it doesn't exist already.
  systemd.services.generate-nix-store-signing-key = {
    wantedBy = ["multi-user.target"];
    serviceConfig.Type = "oneshot";
    path = [pkgs.nix];
    script = ''
      [[ -f /etc/nix/store-signing-key ]] && exit
      nix-store --generate-binary-cache-key \
        linux-builder.orb.local-1 \
        /etc/nix/store-signing-key \
        /etc/nix/store-signing-key.pub
    '';
  };

  system.activationScripts.add-nix-var-cache-lexe-dir = {
    text = ''
      install -m 0755           -d /var/cache
      install -m 2770 -g nixbld -d /var/cache/lexe
      ${pkgs.acl.bin}/bin/setfacl --default -m group:nixbld:rwx /var/cache/lexe
    '';
  };
}
