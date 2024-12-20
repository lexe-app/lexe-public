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
  # Taken from `/etc/nixos/configuration.nix` in a freshly generated NixOS 24.05
  # OrbStack VM.
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
    ./incus.nix
    ./orbstack.nix
  ];

  # OrbStack now generates this `users` block in the `configuration.nix`. I've
  # copied this over, but we'll have to `sed "s/{{ username }}/$USER/"` before
  # activation for this to work.
  users.users."{{ username }}" = {
    uid = 501;
    extraGroups = ["wheel"];

    # simulate isNormalUser, but with an arbitrary UID
    isSystemUser = true;
    group = "users";
    createHome = true;
    home = "/home/{{ username }}";
    homeMode = "700";
    useDefaultShell = true;
  };

  security.sudo.wheelNeedsPassword = false;

  # This being `true` leads to a few nasty bugs, change at your own risk!
  users.mutableUsers = false;

  networking = {
    dhcpcd.enable = false;
    useDHCP = false;
    useHostResolvConf = false;
  };

  systemd.network = {
    enable = true;
    networks."50-eth0" = {
      matchConfig.Name = "eth0";
      networkConfig = {
        DHCP = "ipv4";
        IPv6AcceptRA = true;
      };
      linkConfig.RequiredForOnline = "routable";
    };
  };

  # Extra certificates from OrbStack.
  security.pki.certificates = [
    ''
      -----BEGIN CERTIFICATE-----
      MIICDDCCAbKgAwIBAgIQW3k1qlbV5cRze6aAyG/3WTAKBggqhkjOPQQDAjBmMR0w
      GwYDVQQKExRPcmJTdGFjayBEZXZlbG9wbWVudDEeMBwGA1UECwwVQ29udGFpbmVy
      cyAmIFNlcnZpY2VzMSUwIwYDVQQDExxPcmJTdGFjayBEZXZlbG9wbWVudCBSb290
      IENBMB4XDTIzMTExNjIwMjMxMVoXDTMzMTExNjIwMjMxMVowZjEdMBsGA1UEChMU
      T3JiU3RhY2sgRGV2ZWxvcG1lbnQxHjAcBgNVBAsMFUNvbnRhaW5lcnMgJiBTZXJ2
      aWNlczElMCMGA1UEAxMcT3JiU3RhY2sgRGV2ZWxvcG1lbnQgUm9vdCBDQTBZMBMG
      ByqGSM49AgEGCCqGSM49AwEHA0IABCQw9nJqHN/8b6X680JDmJrFXJ8N5y9AlZOg
      kI6/iBuktdXyiSGhbFPU+l54+JK1XkZ8dKxZsNGaKl+BMl0PYHmjQjBAMA4GA1Ud
      DwEB/wQEAwIBBjAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQWBBQiZeJ6dekQ6wVn
      8ATNUyoBnCpOQjAKBggqhkjOPQQDAgNIADBFAiA6eotAAObTAQgTfd8foMB4qeB3
      tlaVYMa0k2RF6lXhcQIhAIIuDNHV3t7Aj6wLGUQ9qn0we0ePdZ2Cmx9Woj1eE3sd
      -----END CERTIFICATE-----
    ''
  ];

  # This value determines the NixOS release from which the default
  # settings for stateful data, like file locations and database versions
  # on your system were taken. It‘s perfectly fine and recommended to leave
  # this value at the release version of the first install of this system.
  # Before changing this value read the documentation for this option
  # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
  system.stateVersion = "24.11"; # Did you read the comment?

  # As this is intended as a standalone image, undo some of the minimal profile stuff
  documentation = {
    enable = true;
    nixos.enable = true;
    man.enable = true;
  };

  #
  # LEXE LINUX-BUILDER CONFIG
  #

  nix = {
    # Add all members of the `wheel` group to nix trusted-users.
    settings.trusted-users = ["@wheel"];

    # Extra /etc/nix/nix.conf options.
    extraOptions = ''
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
    gc = {
      automatic = true;
      dates = "monthly";
    };
  };

  # Enable git so `nix build` works in git repos.
  programs.git.enable = true;

  # Add some packages to the environment
  environment.systemPackages = [
    pkgs.htop
    # Use with `breakpointHook` to debug broken package builds. The hook will
    # pause the build if it breaks, letting you drop into a container to inspect.
    # See: <https://nixos.org/manual/nixpkgs/stable/#breakpointhook>
    pkgs.cntr
    # `just` command runner
    pkgs.just
    # cat files with syntax highlighting
    pkgs.bat
    # JSON processor; used for releases.json when verifying reproducible builds
    pkgs.jq
  ];

  environment.shellAliases = {
    g = "git";
    j = "just";
  };

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

  system.activationScripts = {
    # setup the shared sccache Rust build cache
    add-nix-var-cache-lexe-dir = {
      text = ''
        install -m 0755           -d /var/cache
        install -m 2770 -g nixbld -d /var/cache/lexe
        ${pkgs.acl.bin}/bin/setfacl --default -m group:nixbld:rwx /var/cache/lexe
      '';
    };

    # use azure credentials from host macOS
    link-macos-azure-credentials = {
      text = ''
        ln -sfn "/mnt/mac/Users/{{ username }}/.azure" "/home/{{ username }}/.azure"
        chown -hP {{ username }}:users "/home/{{ username }}/.azure"
      '';
    };
  };

  # enable fzf fuzzy finder
  programs.fzf = {
    fuzzyCompletion = true;
    # e.g. hit CTRL-R in bash to fuzzy search through bash history
    keybindings = true;
  };

  # vim keybinds in bash
  environment.etc."inputrc".text = ''
    # GNU readline settings
    #
    # ### Inspiration
    #
    # <https://www.topbug.net/blog/2017/07/31/inputrc-for-humans/>
    # <https://github.com/atweiden/dotfiles/blob/master/.inputrc>

    # use vi mode
    set editing-mode vi
    set keymap vi

    # allow UTF-8 input and output, instead of showing stuff like
    # $'\0123\0456'
    set input-meta on
    set output-meta on
    set convert-meta off

    # display possible completions according to $LC_COLORS
    set colored-stats on

    # auto completion ignores case
    set completion-ignore-case on

    # only display 3 characters of the common prefix in the completion
    set completion-prefix-display-length 3

    # display a / after any symlinked directories
    set mark-symlinked-directories on

    # don't ring the bell, but instead show the completions immediately
    set show-all-if-ambiguous on
    set show-all-if-unmodified on

    # append completions by characters that indicate their file type according to
    # stat
    set visible-stats on

    # Be more intelligent when autocompleting by also looking at the text
    # after the cursor. For example, when the current line is "cd
    # ~/src/mozil", and the cursor is on the "z", pressing Tab will not
    # autocomplete it to "cd ~/src/mozillail", but to "cd ~/src/mozilla".
    # (This is supported by the Readline used by Bash 4.)
    set skip-completed-text on
  '';
}
