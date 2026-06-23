#!/usr/bin/env bash
set -euo pipefail

# Lint an Android App Bundle (.aab) for 16KiB ELF LOAD segment alignment.
# 16KiB page-size support is required for Android 15+ targets. Google Play
# is now rejecting all new apps and soon all app updates that do not comply
# with this requirement.
#
# See: <https://developer.android.com/guide/practices/page-sizes>

shopt -s nullglob

readonly MIN_16KB_ALIGN_POWER=14
readonly DEFAULT_AAB_PATH="build/app/outputs/bundle/prodRelease/app-prod-release.aab"

# Print script usage and defaults.
usage() {
  echo >&2 "usage: $0 [path/to/app.aab]"
  echo >&2 "default path: $DEFAULT_AAB_PATH"
}

print_use_devshell() {
  echo >&2 "enter the android dev shell with:"
  echo >&2 ""
  echo >&2 "    nix develop .#app-android"
  echo >&2 ""
}

# Resolve llvm-objdump in precedence order:
# 1) ANDROID_NDK_ROOT toolchain path
# 2) llvm-objdump from PATH
find_llvm_objdump() {
  local ndk_root
  local candidate
  local candidates

  if [[ -n ${ANDROID_NDK_ROOT:-} ]]; then
    ndk_root="$ANDROID_NDK_ROOT"
  else
    ndk_root=""
  fi

  if [[ -n $ndk_root ]]; then
    candidates=("$ndk_root"/toolchains/llvm/prebuilt/*/bin/llvm-objdump)
    for candidate in "${candidates[@]}"; do
      if [[ -x $candidate ]]; then
        echo "$candidate"
        return 0
      fi
    done
  fi

  if command -v llvm-objdump &> /dev/null; then
    command -v llvm-objdump
    return 0
  fi

  return 1
}

# Restrict lint checks to ABIs that Google Play requires to be 16KiB-ready.
is_64_bit_abi() {
  local abi="$1"
  [[ $abi == "arm64-v8a" || $abi == "x86_64" ]]
}

# Accept common alignment formats from llvm-objdump output, and return success
# only when alignment is >= 16KiB.
alignment_is_16kb_or_more() {
  local align="$1"

  if [[ $align =~ ^2\*\*([[:digit:]]+)$ ]]; then
    local power="${BASH_REMATCH[1]}"
    ((power >= MIN_16KB_ALIGN_POWER))
    return $?
  fi

  if [[ $align =~ ^0x([0-9a-fA-F]+)$ ]]; then
    local value="$((16#${BASH_REMATCH[1]}))"
    ((value >= 16384))
    return $?
  fi

  if [[ $align =~ ^[[:digit:]]+$ ]]; then
    local value="$align"
    ((value >= 16384))
    return $?
  fi

  return 2
}

#
# Parse args and validate required inputs/tools.
#

if [[ ${1:-} == "--help" || ${1:-} == "-h" ]]; then
  usage
  exit 0
fi

if [[ $# -gt 1 ]]; then
  usage
  exit 1
fi

aab_path="${1:-$DEFAULT_AAB_PATH}"
if [[ ! -f $aab_path ]]; then
  echo >&2 "error: .aab not found: $aab_path"
  exit 1
fi

if ! command -v unzip &> /dev/null; then
  echo >&2 "error: 'unzip' not found"
  print_use_devshell
  exit 1
fi

llvm_objdump="$(find_llvm_objdump || true)"
if [[ -z $llvm_objdump ]]; then
  echo >&2 "error: couldn't find llvm-objdump in PATH or ANDROID_NDK_ROOT"
  print_use_devshell
  exit 1
fi

#
# Extract the bundle and locate packaged shared objects.
#

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

unzip -qq "$aab_path" -d "$tmp_dir"

mapfile -t shared_objects < <(find "$tmp_dir" -type f -path '*/lib/*/*.so' | sort)

if [[ ${#shared_objects[@]} -eq 0 ]]; then
  echo "FAIL: somehow there are no *.so native libraries in $aab_path"
  exit 1
fi

echo "Checking 16KiB ELF LOAD alignment in: $aab_path"
echo "Using llvm-objdump: $llvm_objdump"

#
# Lint each shared library's LOAD segment alignments and track failures.
#

checked_count=0
failed_count=0
skipped_count=0
failures=()

for so_path in "${shared_objects[@]}"; do
  abi="$(basename "$(dirname "$so_path")")"
  rel_path="${so_path#"$tmp_dir"/}"

  if ! is_64_bit_abi "$abi"; then
    skipped_count=$((skipped_count + 1))
    continue
  fi

  checked_count=$((checked_count + 1))
  mapfile -t load_alignments < <(
    "$llvm_objdump" --private-headers "$so_path" |
      awk '/^[[:space:]]*LOAD[[:space:]]/ { print $NF }'
  )

  if [[ ${#load_alignments[@]} -eq 0 ]]; then
    failed_count=$((failed_count + 1))
    failures+=("$rel_path (no LOAD headers found)")
    echo "FAIL: $rel_path (no LOAD headers found)"
    continue
  fi

  so_ok=1
  for align in "${load_alignments[@]}"; do
    if alignment_is_16kb_or_more "$align"; then
      continue
    fi

    so_ok=0
    break
  done

  if [[ $so_ok -eq 1 ]]; then
    echo "PASS: $rel_path"
  else
    failed_count=$((failed_count + 1))
    failures+=("$rel_path")
    echo "FAIL: $rel_path"
  fi
done

#
# Emit a summary and fail when any 64-bit library is under-aligned.
#

echo
echo "Checked $checked_count 64-bit libraries; skipped $skipped_count non-64-bit libraries."

if [[ $failed_count -gt 0 ]]; then
  echo "Found $failed_count unaligned 64-bit libraries:"
  for failure in "${failures[@]}"; do
    echo "  - $failure"
  done
  exit 1
fi

echo "PASS: all checked 64-bit libraries are 16KiB-aligned."
