# list all available commands
default:
    just --list

diffoscope-cross target:
    #!/usr/bin/env bash
    set -eu -o pipefail

    # build natively
    NATIVE_NIX_STORE_PATHS=$(
      time nix build \
          --print-build-logs \
          --json \
          .#{{ target }} \
          | jq -r '[.[].outputs.out] | join(" ")' \
    )

    # build in x86_64-linux NixOS VM
    VM_NIX_STORE_PATHS=$( \
      time nix build \
        --print-build-logs \
        --json \
        --eval-store auto \
        --store ssh-ng://linux-builder@orb \
        .#packages.x86_64-linux.{{ target }} \
        | jq -r '[.[].outputs.out] | join(" ")' \
    )
    time nix copy \
      --no-check-sigs \
      --from ssh-ng://linux-builder@orb \
      ${VM_NIX_STORE_PATHS}

    # compare binaries
    diffoscope --text-color=always $VM_NIX_STORE_PATHS $NATIVE_NIX_STORE_PATHS \
      | bat
