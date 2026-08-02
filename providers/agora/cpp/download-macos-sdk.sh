#!/usr/bin/env bash
set -euo pipefail

script_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd "$script_root/../../.." && pwd)"
agora_version="4.6.2"
infra_version="1.3.7"
destination="${1:-$repository_root/build/vendor/agora-macos-$agora_version}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "[VOXA][ERROR] The automatic Agora SDK installer currently supports macOS only." >&2
  echo "[VOXA][HELP]  Use the official SDK page: https://docs.agora.io/en/api-reference/sdks?product=voice" >&2
  exit 2
fi
for command_name in curl shasum unzip; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "[VOXA][ERROR] Required command is missing: $command_name" >&2
    exit 2
  fi
done

if [[ -f "$destination/.voxa-agora-sdk-version" ]] &&
   [[ "$(cat "$destination/.voxa-agora-sdk-version")" == "$agora_version" ]]; then
  echo "[VOXA][AGORA] SDK already verified path=$destination version=$agora_version"
  exit 0
fi
if [[ -e "$destination" ]]; then
  echo "[VOXA][ERROR] Refusing to overwrite existing SDK path: $destination" >&2
  exit 2
fi

download_root="$(mktemp -d)"
trap 'rm -rf "$download_root"' EXIT
extract_root="$download_root/extracted"
mkdir -p "$extract_root"

echo "[VOXA][AGORA] Official SDK source: https://github.com/AgoraIO/AgoraRtcEngine_macOS"
echo "[VOXA][AGORA] Downloading official macOS XCFrameworks version=$agora_version"
while IFS='|' read -r component version checksum; do
  [[ -n "$component" ]] || continue
  if [[ "$component" == "aosl" ]]; then
    url="https://download.agora.io/swiftpm/AgoraInfra_macOS/$version/$component.xcframework.zip"
  else
    url="https://download.agora.io/swiftpm/AgoraRtcEngine_macOS/$version/$component.xcframework.zip"
  fi
  archive="$download_root/$component.zip"
  echo "[VOXA][DOWNLOAD] component=$component url=$url"
  curl --fail --location --show-error --silent "$url" --output "$archive"
  actual="$(shasum -a 256 "$archive" | awk '{print $1}')"
  if [[ "$actual" != "$checksum" ]]; then
    echo "[VOXA][ERROR] SHA-256 mismatch component=$component expected=$checksum actual=$actual" >&2
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

printf '%s\n' "$agora_version" > "$extract_root/.voxa-agora-sdk-version"
mkdir -p "$(dirname "$destination")"
mv "$extract_root" "$destination"
echo "[VOXA][AGORA] SDK ready path=$destination version=$agora_version checksums=verified"
