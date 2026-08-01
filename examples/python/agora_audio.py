"""Receive Agora PCM audio as owned Voxa AudioFrame values.

Requires CPython 3.9 and ``agora-python-sdk==3.4.2.1``. Set
VOXA_AGORA_APP_ID, VOXA_AGORA_CHANNEL and optionally VOXA_AGORA_TOKEN before
running. Credentials are read only from the environment and never logged.
"""

import os
import time

from voxa.providers import AgoraRtcClient


app_id = os.environ["VOXA_AGORA_APP_ID"]
channel = os.environ["VOXA_AGORA_CHANNEL"]
token = os.environ.get("VOXA_AGORA_TOKEN", "")

with AgoraRtcClient(app_id) as client:
    client.join(channel, token)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        frame = client.ingress.try_pop()
        if frame is None:
            time.sleep(0.01)
            continue
        print(
            "audio",
            frame.sample_rate_hz,
            frame.channels,
            frame.samples_per_channel,
        )
