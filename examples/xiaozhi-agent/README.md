# Xiaozhi + Muxiva voice agent (Raspberry Pi 4B)

Turn a [Xiaozhi ESP32 board](https://github.com/78/xiaozhi-esp32) into the
client of a Muxiva voice pipeline running on a Raspberry Pi:

```text
ESP32 (Opus/WebSocket)
  │
  ▼
Xiaozhi gateway → Qwen realtime ASR → Pi Agent → speech formatter
       ▲                                  │              │
       │                                  └─ tools       ▼
       └──── paced Opus ← resampler ← Qwen realtime TTS
```

The Graph keeps transport, ASR, Agent policy, formatting, TTS, and playback as
separate Nodes. `@muxiva/agent` supplies the reusable Turn Controller inside the
Agent Node; product routes and Tools come from
[`muxiva-pi-agent`](https://github.com/PiyotaHu/muxiva-pi-agent).

## 1. Responsibilities

| Layer | Owns |
| --- | --- |
| Muxiva Runtime | Frames, Signals, bounded queues, scheduling, lifecycle |
| `@muxiva/agent` | turn admission, cancellation, deadlines, stale-output suppression, Driver recovery, route validation |
| `muxiva-pi-agent` | model session, capability policy, weather/time/search/device/artwork Tools, voice presentation policy |
| Qwen ASR Node | server endpointing, final transcript, filler rejection, validated barge-in |
| Qwen TTS Node | sentence synthesis, cancellation, retry, Turn drain barrier |
| Xiaozhi provider | WebSocket/Opus protocol, jitter buffer, real-time packet pacing, UI protocol mapping |

The Graph is deliberately acyclic. Assistant text is not fed back into ASR;
speaker echo control belongs to device AEC and validated final transcripts.

## 2. Setup

Prerequisites are a 64-bit Raspberry Pi OS, Rust stable, Python 3, Node 22 or
newer, and an ESP32 already flashed with compatible Xiaozhi firmware.

```bash
./setup.sh
```

The setup command creates/reuses the repository `.venv` and installs Pillow for
the artwork conversion and gallery helpers. The service must put that `.venv`
first on `PATH`; the setup command verifies that the image pipeline can import
it before returning successfully.

Configure secrets and deployment-specific endpoints in `.env` rather than in
the committed Graph:

```dotenv
DASHSCOPE_API_KEY=...
DASHSCOPE_WORKSPACE_ID=...
ESP32_HUB_URL=http://127.0.0.1:8890/command
ESP32_HUB_TOKEN=...
MUXIVA_IMAGE_PUBLIC_URL=http://raspberry-pi.local:8890/generated/
```

Optional endpoint overrides are `DASHSCOPE_COMPATIBLE_BASE_URL` and
`DASHSCOPE_SEARCH_ENDPOINT`. The public DashScope endpoints remain defaults.

## 3. Run

```bash
./run.sh
```

The WebSocket server listens on `0.0.0.0:8888`. Configure the firmware endpoint
as `ws://<raspberry-pi-ip>:8888`, then wake the board and speak. A validated new
utterance can interrupt an active reply.

## 4. Capability packs and routing

The `pi-agent` Node in `graph.json` enables independent capability packs:

| Configuration | Capability pack |
| --- | --- |
| `information_tools_enabled` | date/time and current/forecast weather |
| `web_search_enabled` | live web/news search |
| `device_tools_enabled` | speaker volume control |
| `artwork_tools_enabled` | generation, saved-art replay, and gallery |
| `workspace_tools_enabled` | bounded coding workspace access |

The product route grants only the pack required by the utterance. Stable
knowledge takes the model-only fast route. News and current facts require live
search; weather and date questions require their factual Tool. If a required
pack is disabled or its Tool fails, the Turn fails visibly instead of falling
back to an ungrounded model answer.

Timeouts are layered: search has its own bounded timeout, while the whole Agent
Turn has a longer deadline. Spoken progress is disabled by default because it
can introduce an extra TTS session and first-word jitter; applications may opt
in with `progress_message` and `progress_delay_ms`.

## 5. Regression gate

Every change touching ASR, Agent turns, TTS, transport, or Graph wiring must run:

```bash
python3 examples/xiaozhi-agent/tests/run_voice_regression.py
```

Run the live cloud/WebSocket three-turn case in a maintenance window:

```bash
python3 examples/xiaozhi-agent/tests/run_voice_regression.py --live
```

See [tests/VOICE_REGRESSION.md](tests/VOICE_REGRESSION.md) for the bad-case
matrix covering first-word stutter, mid-reply stalls, cancellation, filler
utterances, long ASR turns, decimals, required Tools, and repeated turns.

## 6. Troubleshooting

- Device cannot connect: verify TCP port `8888`, the LAN address, and `ws://`.
- ASR text appears but no answer: inspect `muxiva.agent.route.selected`, Tool
  completion, and `muxiva.agent.response.failed` events.
- Audio starts or stops incorrectly: confirm TTS drain reaches
  `xiaozhi-events`, and run the gateway pacing tests.
- Weather/news is guessed: confirm its capability pack is enabled and the Tool
  completed successfully; required Tools must never silently fall back.
- Drawing reaches the model but no image appears: run
  `.venv/bin/python -c 'from PIL import Image'`, then rerun `setup.sh` if the
  import fails. Recover the temporary generated-image URL from the service log
  before it expires instead of paying for a second generation.
- Wake word is insensitive: this is firmware/microphone/AEC policy, not an
  Agent route. Validate it on the physical board.

Cloud pricing and free quotas change over time. Check the current Alibaba Cloud
Model Studio billing pages before choosing models; this repository does not
embed time-sensitive price claims.
