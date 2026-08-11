#!/usr/bin/env bash
set -euo pipefail

# Run Flutter integration tests in a fresh headless Android emulator.

# A unique AVD name lets us find this run's device on the shared ADB server.
readonly AVD_NAME="lexe-integration-$$"
readonly BOOT_TIMEOUT_SECONDS=240

adb=""
emulator_pid=""
serial=""
tmp_dir=""

print_use_devshell() {
  echo >&2 "enter the Android dev shell with:"
  echo >&2 ""
  echo >&2 "    nix develop .#app-android-with-emulator"
  echo >&2 ""
}

# Stop the emulator and remove its temporary AVD on exit
cleanup() {
  local status=$?

  trap - EXIT INT TERM

  if [[ -n $serial ]]; then
    timeout 5 "$adb" -s "$serial" emu kill &> /dev/null || true
  fi
  if [[ -n $emulator_pid ]]; then
    kill -KILL "$emulator_pid" &> /dev/null || true
    wait "$emulator_pid" 2> /dev/null || true
  fi

  if [[ -n $tmp_dir && -d $tmp_dir ]]; then
    rm -rf "$tmp_dir"
  fi

  exit "$status"
}

# Find a named Android SDK command-line tool
find_commandline_tool() {
  local tool="$1"
  local tool_path

  tool_path="$(find "$ANDROID_EMULATOR_SDK_ROOT/cmdline-tools" \
    -path "*/bin/$tool" -type f -print -quit)"
  if [[ -z $tool_path ]]; then
    echo >&2 "error: '$tool' not found in the emulator SDK"
    print_use_devshell
    exit 1
  fi

  echo "$tool_path"
}

# Find this run's emulator serial from its unique AVD name
find_emulator_serial() {
  local avd_name
  local candidate

  while read -r candidate _; do
    if [[ $candidate != emulator-* ]]; then
      continue
    fi

    avd_name="$(
      "$adb" -s "$candidate" emu avd name 2> /dev/null |
        sed -n '1{s/\r$//;p;}' || true
    )"
    if [[ $avd_name == "$AVD_NAME" ]]; then
      serial="$candidate"
      return 0
    fi
  done < <("$adb" devices | tail -n +2)

  return 1
}

# Wait for the emulator to appear in ADB and finish booting
wait_for_boot() {
  local deadline=$((SECONDS + BOOT_TIMEOUT_SECONDS))

  echo "Waiting for $AVD_NAME to boot..."
  until [[ -n $serial && $("$adb" -s "$serial" shell getprop sys.boot_completed 2> /dev/null | tr -d '\r') == 1 ]]; do
    if ! kill -0 "$emulator_pid" 2> /dev/null; then
      echo >&2 "error: Android emulator exited during boot"
      cat >&2 "$tmp_dir/emulator.log"
      return 1
    fi

    if [[ -z $serial ]]; then
      find_emulator_serial || true
    fi

    if ((SECONDS >= deadline)); then
      echo >&2 "error: Android emulator did not boot within $BOOT_TIMEOUT_SECONDS seconds"
      cat >&2 "$tmp_dir/emulator.log"
      return 1
    fi

    sleep 2
  done

  echo "$AVD_NAME booted as $serial"
}

# Validate the host, boot an AVD, and run the integration tests
main() {
  if [[ -z ${ANDROID_SDK_ROOT:-} ||
    -z ${ANDROID_EMULATOR_SDK_ROOT:-} ||
    -z ${LEXE_ANDROID_EMULATOR_SYSTEM_IMAGE:-} ]]; then
    echo >&2 "error: Android SDK environment is not configured"
    print_use_devshell
    exit 1
  fi

  if [[ $(uname -s) == "Linux" && ! -w /dev/kvm ]]; then
    echo >&2 "error: headless Android tests require writable /dev/kvm"
    echo >&2 "suggestion:"
    echo >&2 ""
    echo >&2 "  sudo usermod -aG kvm $USER"
    echo >&2 "  sudo chmod 0660 /dev/kvm"
    echo >&2 ""
    exit 1
  fi

  local avdmanager
  local build_sdk="$ANDROID_SDK_ROOT"
  local emulator
  local emulator_sdk="$ANDROID_EMULATOR_SDK_ROOT"

  adb="$build_sdk/platform-tools/adb"
  emulator="$emulator_sdk/emulator/emulator"
  avdmanager="$(find_commandline_tool avdmanager)"

  if [[ ! -x $adb || ! -x $emulator ]]; then
    echo >&2 "error: Android emulator tools are missing"
    print_use_devshell
    exit 1
  fi

  tmp_dir="$(mktemp -d)"
  export ANDROID_USER_HOME="$tmp_dir/android-user"
  export ANDROID_AVD_HOME="$ANDROID_USER_HOME/avd"
  mkdir -p "$ANDROID_AVD_HOME"

  # Only AVD state is per-run; the multi-GiB system image stays shared.
  echo no | env \
    ANDROID_HOME="$emulator_sdk" \
    ANDROID_SDK_ROOT="$emulator_sdk" \
    "$avdmanager" create avd \
    --force \
    --name "$AVD_NAME" \
    --package "$LEXE_ANDROID_EMULATOR_SYSTEM_IMAGE" \
    --device medium_phone

  "$adb" start-server &> /dev/null

  env \
    ANDROID_HOME="$emulator_sdk" \
    ANDROID_SDK_ROOT="$emulator_sdk" \
    "$emulator" \
    -avd "$AVD_NAME" \
    -accel on \
    -gpu swiftshader \
    -no-audio \
    -no-boot-anim \
    -no-metrics \
    -no-snapshot \
    -no-window \
    > "$tmp_dir/emulator.log" 2>&1 &
  emulator_pid=$!

  wait_for_boot

  "$adb" -s "$serial" shell settings put global window_animation_scale 0
  "$adb" -s "$serial" shell settings put global transition_animation_scale 0
  "$adb" -s "$serial" shell settings put global animator_duration_scale 0

  echo "Running integration tests on $serial"
  flutter test integration_test -d "$serial" "$@"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

main "$@"
