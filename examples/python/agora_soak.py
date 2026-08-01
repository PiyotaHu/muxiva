"""Credential-driven Agora long-run certification without logging secrets."""

import json
import os
from pathlib import Path
import time
from typing import Optional

from voxa.providers import AgoraRtcClient


def _read_token(path: Optional[Path]) -> str:
    if path is None:
        return os.environ.get("VOXA_AGORA_TOKEN", "")
    return path.read_text(encoding="utf-8").strip()


app_id = os.environ["VOXA_AGORA_APP_ID"]
channel = os.environ["VOXA_AGORA_CHANNEL"]
duration = max(1, int(os.environ.get("VOXA_AGORA_SOAK_SECONDS", "600")))
token_path_text = os.environ.get("VOXA_AGORA_TOKEN_FILE")
token_path = Path(token_path_text) if token_path_text else None
token = _read_token(token_path)
token_mtime = token_path.stat().st_mtime_ns if token_path else None
connected = False
audio_frames = 0
control_events = 0

with AgoraRtcClient(app_id, event_capacity=256) as client:
    client.join(channel, token)
    deadline = time.monotonic() + duration
    while time.monotonic() < deadline:
        while (event := client.try_pop_event()) is not None:
            control_events += 1
            if event.kind == "connection_state" and event.state == 3:
                connected = True
        while client.ingress.try_pop() is not None:
            audio_frames += 1
        if token_path is not None:
            current_mtime = token_path.stat().st_mtime_ns
            if current_mtime != token_mtime:
                client.renew_token(_read_token(token_path))
                token_mtime = current_mtime
        time.sleep(0.01)
    stats = client.rtc_stats
    ingress = client.ingress.stats

summary = {
    "duration_seconds": duration,
    "connected": connected,
    "audio_frames": audio_frames,
    "control_events": control_events,
    "connection_epoch": stats.connection_epoch,
    "reconnects": stats.reconnects,
    "connection_losses": stats.connection_losses,
    "token_renewals": stats.token_renewals,
    "event_drops": stats.events_dropped,
    "audio_drops": ingress.full_dropped,
}
print(json.dumps(summary, sort_keys=True))
if not connected:
    raise SystemExit("Agora soak failed: no connected callback")
if os.environ.get("VOXA_AGORA_REQUIRE_AUDIO") == "1" and audio_frames == 0:
    raise SystemExit("Agora soak failed: no remote audio frames")
