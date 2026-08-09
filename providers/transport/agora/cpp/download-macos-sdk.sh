#!/usr/bin/env bash
set -euo pipefail

script_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_root/../../.." && pwd)"
agora_version="4.6.2"
infra_version="1.3.7"
destination="${1:-$repository_root/build/vendor/agora-macos-$agora_version}"
version_marker="$destination/.muxiva-agora-sdk-version"
legacy_version_marker="$destination/.voxa-agora-sdk-version"

sdk_components_are_present() {
  local component
  for component in \
    AgoraRtcKit \
    Agorafdkaac \
    Agoraffmpeg \
    AgoraSoundTouch \
    video_dec \
    aosl; do
    if [[ ! -f "$destination/$component.xcframework/Info.plist" ]]; then
      return 1
    fi
  done
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "[MUXIVA][ERROR] The automatic Agora SDK installer currently supports macOS only." >&2
  echo "[MUXIVA][HELP]  Use the official SDK page: https://docs.agora.io/en/api-reference/sdks?product=voice" >&2
  exit 2
fi
for command_name in curl shasum unzip; do
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
if [[ -f "$legacy_version_marker" ]] &&
   [[ "$(cat "$legacy_version_marker")" == "$agora_version" ]] &&
   sdk_components_are_present; then
  printf '%s\n' "$agora_version" > "$version_marker"
  echo "[MUXIVA][AGORA][REUSE] Existing SDK from the Voxa-to-Muxiva rename is valid; download skipped."
  echo "[MUXIVA][AGORA][MIGRATE] Added Muxiva verification marker path=$version_marker"
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
extract_root="$download_root/extracted"
mkdir -p "$extract_root"

echo "[MUXIVA][AGORA] Official SDK source: https://github.com/AgoraIO/AgoraRtcEngine_macOS"
echo "[MUXIVA][AGORA] Downloading official macOS XCFrameworks version=$agora_version"
while IFS='|' read -r component version checksum; do
  [[ -n "$component" ]] || continue
  if [[ "$component" == "aosl" ]]; then
    url="https://download.agora.io/swiftpm/AgoraInfra_macOS/$version/$component.xcframework.zip"
  else
    url="https://download.agora.io/swiftpm/AgoraRtcEngine_macOS/$version/$component.xcframework.zip"
  fi
  archive="$download_root/$component.zip"
  echo "[MUXIVA][DOWNLOAD] component=$component url=$url"
  curl --fail --location --show-error --silent "$url" --output "$archive"
  actual="$(shasum -a 256 "$archive" | awk '{print $1}')"
  if [[ "$actual" != "$checksum" ]]; then
    echo "[MUXIVA][ERROR] SHA-256 mismatch component=$component expected=$checksum actual=$actual" >&2
    exit 1
  fi
  unzip -q "$archive" -d "$extract_root"
done <<EOF
AgoraRtcKit|$agora_version|189aaee1d4cb8f3567dc4251098f77e84c2d2fb4b39067c2a6731aae2174b31a
Agorafdkaac|$agora_version|eb1235366e9b952a71163afeada2fe350f60dca050e866f0b5c1bb0411640ca8
Agoraffmpeg|$agora_version|ca8fd0f7d008d2398c3616e28dce66b78ab23beb2d1cfcf7d29a5a0d3b7105e3
AgoraSoundTouch|$agora_version|fa35927bef8acb16caa774e7c3a2fcc2b27292a03f99a6fa792f28bd90a297f4
video_dec|$agora_version|2fabeed4a4dca155cce6ce796e9d561ec9b76c8e273b247260c40301402615b5
aosl|$infra_version|8d7513a081d0ece099071a283622ec109b5facdabeff9da559cd7f5649a110eb
EOF

printf '%s\n' "$agora_version" > "$extract_root/.muxiva-agora-sdk-version"
mkdir -p "$(dirname "$destination")"
mv "$extract_root" "$destination"
echo "[MUXIVA][AGORA] SDK ready path=$destination version=$agora_version checksums=verified"
