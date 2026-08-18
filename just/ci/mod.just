# CI task entry points, invoked as `just ci <recipe>` from the repo root.
#
# These are run by GitHub Actions; see `.github/workflows/ci.yml`.

set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '../..'

shfmt-config := "--indent 2 --simplify --space-redirects --language-dialect bash"

# Select a recipe interactively.
default:
    @just --choose

bash-fmt-check:
    nix develop .#bash-lint --command \
      fd --extension "sh" --exec-batch \
        shfmt {{ shfmt-config }} --diff

bash-lint:
    nix develop .#bash-lint --command \
      fd --extension "sh" --exec-batch \
        shellcheck --shell=bash

# Check that `cargo check` passes on every commit in the PR
check-all-commits pr-base-hash="" pr-head-hash="":
    PR_BASE_HASH="{{ pr-base-hash }}" PR_HEAD_HASH="{{ pr-head-hash }}" \
      ./just/ci/check-all-commits.sh

# Check commit messages: no Claude attribution, lines fit in 72 chars
check-commits-fmt pr-base-hash="" pr-head-hash="":
    PR_BASE_HASH="{{ pr-base-hash }}" PR_HEAD_HASH="{{ pr-head-hash }}" \
      ./just/ci/check-commits-no-claude.sh
    PR_BASE_HASH="{{ pr-base-hash }}" PR_HEAD_HASH="{{ pr-head-hash }}" \
      ./just/ci/check-commits-width.sh

just-fmt-check:
    just --fmt --unstable --check

nix-lint:
    nix flake show \
        --show-trace \
        --option allow-import-from-derivation true
    nix develop .#nix-lint --command statix check
