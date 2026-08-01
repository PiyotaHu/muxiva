#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python_bin=${VOXA_AGORA_PYTHON:-"$repo/target/agora-python-probe/bin/python"}
if [ ! -x "$python_bin" ]; then
  echo "SKIP Agora Python SDK: set VOXA_AGORA_PYTHON to a CPython 3.9 environment"
  exit 0
fi
"$python_bin" - <<'PY'
import agorartc
from voxa.providers.agora import AgoraAudioIngress, AgoraRtcClient

assert hasattr(agorartc, "AudioFrameObserver")
assert hasattr(agorartc, "VideoFrameObserver")
observer = AgoraAudioIngress().create_observer(agorartc)
assert isinstance(observer, agorartc.AudioFrameObserver)
client = AgoraRtcClient("0" * 32, agora=agorartc)
assert client.rtc_stats.connection_epoch == 0
client.close()
print("Agora Python engine, observer, and lifecycle probe passed")
PY
