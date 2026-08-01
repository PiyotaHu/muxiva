#!/bin/sh
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python_bin=${VOXA_AGORA_PYTHON:-"$repo/target/agora-python-probe/bin/python"}
if [ ! -x "$python_bin" ]; then
  echo "SKIP Agora Python SDK: set VOXA_AGORA_PYTHON to a CPython 3.9 environment"
  exit 0
fi
"$python_bin" -c 'import agorartc; assert hasattr(agorartc, "AudioFrameObserver"); assert hasattr(agorartc, "VideoFrameObserver"); from voxa.providers.agora import AgoraAudioIngress; observer = AgoraAudioIngress().create_observer(agorartc); assert isinstance(observer, agorartc.AudioFrameObserver); engine = agorartc.createRtcEngineBridge(); assert engine is not None; engine.release(); print("Agora Python engine and Voxa observer probe passed")'
