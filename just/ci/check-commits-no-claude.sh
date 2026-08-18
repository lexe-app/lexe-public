#!/usr/bin/env bash
set -euo pipefail

# # check-commits-no-claude
#
# Checks that no commit messages or author/committer metadata in a PR contain
# Claude Code attribution. Checks commit messages for robot emoji, "Generated
# with [Claude Code]", and "noreply@anthropic.com". Also checks author/committer
# names for "Claude" and emails for "noreply@anthropic.com".
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

# Get all commit messages in the PR
COMMITS=$(git rev-list --no-merges "${PR_BASE_HASH}..${PR_HEAD_HASH}")

EXIT_CODE=0

for commit in ${COMMITS}; do
  COMMIT_MSG=$(git log -1 --format=%B "${commit}")
  COMMIT_SHORT=$(git log -1 --format=%h "${commit}")
  COMMIT_SUBJECT=$(git log -1 --format=%s "${commit}")
  AUTHOR_NAME=$(git log -1 --format=%an "${commit}")
  AUTHOR_EMAIL=$(git log -1 --format=%ae "${commit}")
  COMMITTER_NAME=$(git log -1 --format=%cn "${commit}")
  COMMITTER_EMAIL=$(git log -1 --format=%ce "${commit}")

  # Check for robot emoji
  if echo "${COMMIT_MSG}" | grep -qF "🤖"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) contains forbidden pattern: 🤖"
    EXIT_CODE=1
  fi

  # Check for "Generated with [Claude Code]"
  if echo "${COMMIT_MSG}" | grep -qiF "Generated with [Claude Code]"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) contains forbidden pattern: Generated with [Claude Code]"
    EXIT_CODE=1
  fi

  # Check for noreply@anthropic.com anywhere in the message
  if echo "${COMMIT_MSG}" | grep -qi "noreply@anthropic\.com"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) contains forbidden pattern: noreply@anthropic.com"
    EXIT_CODE=1
  fi

  # Check for "Claude" in author name (case-insensitive)
  if echo "${AUTHOR_NAME}" | grep -qi "claude"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) has forbidden author name: ${AUTHOR_NAME}"
    EXIT_CODE=1
  fi

  # Check for noreply@anthropic.com in author email
  if echo "${AUTHOR_EMAIL}" | grep -qi "noreply@anthropic\.com"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) has forbidden author email: ${AUTHOR_EMAIL}"
    EXIT_CODE=1
  fi

  # Check for "Claude" in committer name (case-insensitive)
  if echo "${COMMITTER_NAME}" | grep -qi "claude"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) has forbidden committer name: ${COMMITTER_NAME}"
    EXIT_CODE=1
  fi

  # Check for noreply@anthropic.com in committer email
  if echo "${COMMITTER_EMAIL}" | grep -qi "noreply@anthropic\.com"; then
    echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) has forbidden committer email: ${COMMITTER_EMAIL}"
    EXIT_CODE=1
  fi
done

if [[ ${EXIT_CODE} -eq 0 ]]; then
  echo "✅ All commits are clean (no Claude Code attribution found)"
else
  echo ""
  echo "ERROR: Some commits contain Claude Code attribution."
  echo "Please remove the robot emoji, 'Generated with [Claude Code]' text,"
  echo "and 'noreply@anthropic.com' email from commit messages and metadata."
  echo ""
  echo "Commits must be attributed to a real identity, not Claude Code. Any"
  echo "sensible identity works: on many machines a Lexe developer is already the"
  echo "global git default, which is fine to keep. If there's no other git identity"
  echo "configured, or if you're unsure, set the global config to 'Lexe Agent':"
  echo '  git config --global user.name "Lexe Agent"'
  echo '  git config --global user.email "noreply@lexe.app"'
  echo ""
  echo "Then re-stamp the offending commits with your configured identity (this"
  echo "also opens the editor so you can strip any attribution from the message)."
  echo ""
  echo "You can amend the last commit with:"
  echo "  git commit --amend --reset-author"
  echo ""
  echo "For older commits, use interactive rebase:"
  echo "  git rebase -i ${PR_BASE_HASH}"
  echo "  # Mark commits as 'edit', then for each:"
  echo "  git commit --amend --reset-author"
  echo "  git rebase --continue"
fi

exit ${EXIT_CODE}
