#!/usr/bin/env bash
set -euo pipefail

# # check-commits-width
#
# Checks that all commit message lines in a PR fit within 72 characters,
# the conventional width for commit message bodies. Lines inside ```
# fenced code blocks are exempt, since code, command output, and URLs
# often can't wrap.
#
# ## Args
#
# PR_BASE_HASH = the base commit to start checking from.
#                defaults to the current `master` branch commit hash.
# PR_HEAD_HASH = the PR HEAD commit to start checking from.
#                defaults to the current `HEAD` commit hash.

MAX_WIDTH=72
FENCE_REGEX='^[[:space:]]*```'

if [[ -z ${PR_BASE_HASH:-} ]]; then
  PR_BASE_HASH="$(git rev-parse master)"
fi

if [[ -z ${PR_HEAD_HASH:-} ]]; then
  PR_HEAD_HASH="$(git rev-parse HEAD)"
fi

echo "PR_BASE_HASH=${PR_BASE_HASH}"
echo "PR_HEAD_HASH=${PR_HEAD_HASH}"

# Get all commits in the PR
COMMITS=$(git rev-list --no-merges "${PR_BASE_HASH}..${PR_HEAD_HASH}")

FAILED_COMMITS=()

for commit in ${COMMITS}; do
  COMMIT_SHORT=$(git log -1 --format=%h "${commit}")
  COMMIT_SUBJECT=$(git log -1 --format=%s "${commit}")

  # Walk the message line by line, tracking ``` code blocks.
  in_code_block=0
  line_num=0
  commit_ok=1
  while IFS= read -r line; do
    line_num=$((line_num + 1))

    if [[ ${line} =~ ${FENCE_REGEX} ]]; then
      in_code_block=$((1 - in_code_block))
      continue
    fi

    if [[ ${in_code_block} -eq 0 && ${#line} -gt ${MAX_WIDTH} ]]; then
      if [[ ${commit_ok} -eq 1 ]]; then
        echo "❌ Commit ${COMMIT_SHORT} (${COMMIT_SUBJECT}) has lines over ${MAX_WIDTH} chars:"
        commit_ok=0
        FAILED_COMMITS+=("${COMMIT_SHORT} (${COMMIT_SUBJECT})")
      fi
      echo "   line ${line_num} (${#line} chars): ${line}"
    fi
  done < <(git log -1 --format=%B "${commit}")
done

if [[ ${#FAILED_COMMITS[@]} -eq 0 ]]; then
  echo "✅ All commit message lines fit within ${MAX_WIDTH} chars"
  exit 0
fi

echo ""
echo "ERROR: These commits have message lines over ${MAX_WIDTH} characters:"
for failed_commit in "${FAILED_COMMITS[@]}"; do
  echo "  ${failed_commit}"
done
echo ""
echo "Rewrap the offending lines to ${MAX_WIDTH} chars, or move content that"
echo "can't wrap (e.g. code, command output, URLs) into a \`\`\` code block,"
echo "which this check exempts."

exit 1
