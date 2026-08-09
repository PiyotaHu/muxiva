#!/usr/bin/env bash
set -euo pipefail

application_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$application_root/../.." && pwd)"

if [[ -n "${MUXIVA_BIN:-}" ]]; then
  if [[ ! -x "$MUXIVA_BIN" ]]; then
    echo "[MUXIVA][ERROR] MUXIVA_BIN is not executable: $MUXIVA_BIN" >&2
    exit 2
  fi
  muxiva_binary="$MUXIVA_BIN"
elif [[ -x "$repository_root/target/debug/muxiva" ]]; then
  muxiva_binary="$repository_root/target/debug/muxiva"
elif [[ -x "$repository_root/target/release/muxiva" ]]; then
  muxiva_binary="$repository_root/target/release/muxiva"
elif command -v muxiva >/dev/null 2>&1; then
  muxiva_binary="$(command -v muxiva)"
else
  echo "The muxiva binary is not installed. Install a release binary or run: cargo install --path crates/muxiva-cli --locked" >&2
  exit 2
fi

case "$(uname -s)" in
  Darwin) native_extension="dylib" ;;
  Linux) native_extension="so" ;;
  *) native_extension="" ;;
esac
native_audio_source="$application_root/.muxiva/native/agora_audio_source/libmuxiva_node_pack${native_extension:+.$native_extension}"
native_audio_source_code="$repository_root/providers/transport/agora/cpp/nodes/agora_audio_source/node.cpp"
cpp_sdk_header="$repository_root/cpp/include/muxiva/muxiva.hpp"
c_abi_header="$repository_root/cpp/include/muxiva/muxiva.h"
if [[ -n "$native_extension" && -f "$native_audio_source" ]] &&
   [[ "$native_audio_source_code" -nt "$native_audio_source" ||
      "$cpp_sdk_header" -nt "$native_audio_source" ||
      "$c_abi_header" -nt "$native_audio_source" ]]; then
  echo '[MUXIVA][ERROR] The installed Agora Node Packs are older than their source or Muxiva ABI headers.' >&2
  echo '[MUXIVA][WHY]   Running a stale native library can hide fixes or report an ABI/factory version mismatch.' >&2
  echo '[MUXIVA][NEXT]  Stop the existing Studio process, then run ./examples/voice-agent/setup.sh once to rebuild every Node Pack.' >&2
  exit 2
fi

if [[ -x "$application_root/.muxiva/venv/bin/python" ]]; then
  export MUXIVA_PYTHON="$application_root/.muxiva/venv/bin/python"
fi
if [[ -n "${MUXIVA_NODE:-}" ]]; then
  export MUXIVA_NODE
fi

echo '[MUXIVA][WELCOME] Real voice setup requires two connection cards: Agora RTC and Alibaba Cloud Model Studio.'
echo '[MUXIVA][STEP 1] Studio opens now. Click Connections in the top toolbar.'
echo '[MUXIVA][STEP 2] Fill missing Required fields once and click Save connections. Studio persists them in this project .env.'
echo '[MUXIVA][STEP 3] Do not click Run or Voice Room until both cards show Ready.'
echo '[MUXIVA][HELP]  https://piyotahu.github.io/muxiva/voice-demo/'
echo "[MUXIVA][CLI]   $muxiva_binary"
echo "[MUXIVA][LOG]   $application_root/.muxiva/runtime.log"

studio_args=("$application_root/graph.json" "$@")
if [[ "$(uname -s)" == "Linux" && -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  has_no_open=false
  for argument in "$@"; do
    if [[ "$argument" == "--no-open" ]]; then
      has_no_open=true
      break
    fi
  done
  if [[ "$has_no_open" == false ]]; then
    studio_args+=("--no-open")
  fi
  echo '[MUXIVA][HEADLESS] No desktop session detected; browser auto-open is disabled.'
  echo '[MUXIVA][HEADLESS] For SSH access, restart with --port 5678 and forward 127.0.0.1:5678 from your laptop.'
  echo '[MUXIVA][HEADLESS] Guide: https://piyotahu.github.io/muxiva/remote-studio/'
fi

mkdir -p "$application_root/.muxiva"
run_lock="$application_root/.muxiva/studio.lock"
if ! mkdir "$run_lock" 2>/dev/null; then
  existing_pid="$(sed -n '1p' "$run_lock/pid" 2>/dev/null || true)"
  if [[ "$existing_pid" =~ ^[0-9]+$ ]] && kill -0 "$existing_pid" 2>/dev/null; then
    echo "[MUXIVA][ERROR] This voice project is already running (pid=$existing_pid)." >&2
    echo "[MUXIVA][NEXT]  Use the existing Studio window, or stop that process before starting another." >&2
    exit 2
  fi
  rm -f "$run_lock/pid"
  rmdir "$run_lock" 2>/dev/null || true
  mkdir "$run_lock"
fi
printf '%s\n' "$$" > "$run_lock/pid"
release_run_lock() {
  rm -f "$run_lock/pid"
  rmdir "$run_lock" 2>/dev/null || true
}
trap release_run_lock EXIT INT TERM
set +e
"$muxiva_binary" studio "${studio_args[@]}" 2>&1 \
  | tee "$application_root/.muxiva/runtime.log"
status=${PIPESTATUS[0]}
set -e
exit "$status"
