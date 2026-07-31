#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_file="$repository_root/cpp/tests/harness/native_tsan_contract.cc"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "SKIP native TSan: the supported quality job is Linux-only"
  exit 0
fi
if [[ ! -f "$source_file" ]]; then
  echo "SKIP native TSan: pure-C++ race contract is not present yet; Rust-linked TSan is unsupported"
  exit 0
fi
if ! command -v clang++ >/dev/null 2>&1; then
  echo "SKIP native TSan: clang++ is unavailable"
  exit 0
fi

output="$repository_root/target/native-tsan-contract"
clang++ -std=c++17 -Wall -Wextra -Werror -fsanitize=thread -fno-omit-frame-pointer \
  "$source_file" -o "$output" -fsanitize=thread
"$output"
