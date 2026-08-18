#!/usr/bin/env bash
set -euo pipefail

# # check-all-commits
#
# Runs some checks on every commit in a PR.
#
# ## Checks
#
# * Commit messages don't contain the git commit template
#
# * `cargo check --locked --workspace --all-targets`
#   This check ensures each commit in a PR at least compiles, so we can
#   easily `git bisect`.
#
# ## Args
#
# PR_BASE_HASH = the base commit to start checking from.
#                defaults to the current `master` branch commit hash.
# PR_HEAD_HASH = the PR HEAD commit to start checking from.
#                defaults to the current `HEAD` commit hash.

if [[ -z ${PR_BASE_HASH} ]]; then
  PR_BASE_HASH="$(git rev-parse master)"
fi

if [[ -z ${PR_HEAD_HASH} ]]; then
  PR_HEAD_HASH="$(git rev-parse HEAD)"
fi

echo "PR_BASE_HASH=${PR_BASE_HASH}"
echo "PR_HEAD_HASH=${PR_HEAD_HASH}"

# Warnings are OK, we just want be sure commits at least compile
unset RUSTFLAGS
unset RUSTDOCFLAGS

# Don't waste time building lsp/node in supervisor/runner build.rs.
# Skip bitcoind and electrs downloads.
export LEXE_SKIP_BUILDSCRIPT=1
export BITCOIND_SKIP_DOWNLOAD=1
export ELECTRSD_SKIP_DOWNLOAD=1

# Reverse the order of commits with `tac`, otherwise we iterate backwards
COMMITS=$(git rev-list --no-merges "${PR_BASE_HASH}..${PR_HEAD_HASH}" | tac)

# Check that commit messages don't contain git commit template gunk
for commit in ${COMMITS}; do
  msg=$(git log -1 --format=%B "${commit}")
  if echo "${msg}" | grep -qF '# Please enter the commit message for your changes.'; then
    echo >&2 "ERROR: Commit ${commit} contains the git commit template in its message."
    echo >&2 "The author's git config likely has 'cleanup = keep' or similar."
    echo >&2 ""
    echo >&2 "--- commit message ---"
    echo >&2 "${msg}"
    echo >&2 "--- end ---"
    exit 1
  fi
done

tmpdir="$(mktemp -d)"
trap 'git worktree remove --force "${tmpdir}"' EXIT

echo "Running per-commit checks in temp worktree: ${tmpdir}"
git worktree add --detach "${tmpdir}" "${PR_HEAD_HASH}"

for commit in ${COMMITS}; do
  # Check in the temp worktree, leaving the caller's worktree untouched.
  (
    set -eux
    git -C "${tmpdir}" checkout "${commit}"
    cd "${tmpdir}"
    cargo check --locked --workspace --all-targets
  )
done
