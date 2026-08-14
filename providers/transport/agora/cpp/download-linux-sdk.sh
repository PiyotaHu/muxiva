#!/usr/bin/env bash
set -euo pipefail

script_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_root/../../../.." && pwd)"
agora_version="4.4.32"
agora_artifact="Agora-RTC-x86_64-linux-gnu-v4.4.32-20250425_144419-675648.tgz"
agora_checksum="9a71c4a5d6fca717e0cdb0bb407086b266466977d10c987ff7d6ba0e3fc14f38"
destination="${1:-$repository_root/build/vendor/agora-linux-$agora_version}"
version_marker="$destination/.muxiva-agora-sdk-version"

sdk_components_are_present() {
  [[ -f "$destination/agora_sdk/include/IAgoraService.h" ]] &&
    [[ -f "$destination/agora_sdk/include/NGIAgoraRtcConnection.h" ]] &&
    [[ -f "$destination/agora_sdk/libagora_rtc_sdk.so" ]]
}

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "[MUXIVA][ERROR] The automatic Linux Agora SDK installer runs on Linux only." >&2
  echo "[MUXIVA][HELP]  Use the official SDK page: https://docs.agora.io/en/api-reference/sdks?product=server-gateway&platform=linux" >&2
  exit 2
fi
for command_name in curl sha256sum tar; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "[MUXIVA][ERROR] Required command is missing: $command_name" >&2
    exit 2
  fi
done

if [[ -f "$version_marker" ]] &&
   [[ "$(cat "$version_marker")" == "$agora_version" ]] &&
   sdk_components_are_present; then
  echo "[MUXIVA][AGORA][REUSE] SDK is already installed and verified; download skipped."
  echo "[MUXIVA][AGORA][REUSE] path=$destination version=$agora_version"
  exit 0
fi
if [[ -e "$destination" ]]; then
  echo "[MUXIVA][ERROR] Existing Agora SDK directory is incomplete or cannot be verified: $destination" >&2
  echo "[MUXIVA][HELP]  Keep it as a backup by renaming it, then rerun setup.sh to download a verified copy." >&2
  echo "[MUXIVA][HELP]  Or pass the absolute path of another complete Agora SDK to setup.sh." >&2
  exit 2
fi

download_root="$(mktemp -d)"
trap 'rm -rf "$download_root"' EXIT
archive="$download_root/$agora_artifact"
url="https://download.agora.io/rtsasdk/release/$agora_artifact"

echo "[MUXIVA][AGORA] Official SDK source: https://docs.agora.io/en/api-reference/sdks?product=server-gateway&platform=linux"
echo "[MUXIVA][AGORA] Downloading official Linux Server Gateway SDK version=$agora_version"
echo "[MUXIVA][DOWNLOAD] url=$url"
curl --fail --location --show-error --silent "$url" --output "$archive"
actual="$(sha256sum "$archive" | awk '{print $1}')"
if [[ "$actual" != "$agora_checksum" ]]; then
  echo "[MUXIVA][ERROR] SHA-256 mismatch expected=$agora_checksum actual=$actual" >&2
  exit 1
fi

extract_root="$download_root/extracted"
mkdir -p "$extract_root"
# The archive nests everything under agora_rtc_sdk/; strip it so the SDK root
# contains agora_sdk/ directly.
tar -xzf "$archive" -C "$extract_root" --strip-components=1

printf '%s\n' "$agora_version" > "$extract_root/.muxiva-agora-sdk-version"
mkdir -p "$(dirname "$destination")"
mv "$extract_root" "$destination"
echo "[MUXIVA][AGORA] SDK ready path=$destination version=$agora_version checksum=verified"
