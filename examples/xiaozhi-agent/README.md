# Xiaozhi + Muxiva voice agent (Raspberry Pi 4B)

Turn a [Xiaozhi ESP32 board](https://github.com/78/xiaozhi-esp32) into the
client of a Muxiva voice pipeline running on a Raspberry Pi 4B:

```
ESP32 (Opus over WebSocket)
        │  ws://<pi-ip>:8888
        ▼
xiaozhi.audio_source ──► qwen.asr_realtime ──► builtin.llm_openai_compatible
   (VAD, barge-in)        (server VAD + ASR)         (DeepSeek/OpenAI/Ollama)
        ▲                                                     │
        │                                                     ▼
xiaozhi.audio_sink ◄── builtin.audio_resampler ◄── qwen.tts_realtime
        ▲                        │                     ▲
        └────────────────────────┴── builtin.speech_formatter
```

- **VAD**: Qwen server VAD (`qwen.asr_realtime`) detects speech start/stop and
  emits barge-in Signals. The built-in `builtin.audio_vad` is a zero-cost local
  CPU alternative if you want VAD to run fully on the Pi.
- **ASR / TTS**: Qwen realtime cloud models by default (fast, no Pi CPU cost).
  They are vendor adapter Nodes; swap them for local `faster-whisper` / `piper`
  provider Nodes when available.
- **LLM**: the vendor-neutral `builtin.llm_openai_compatible` framework Node.
  Point it at DeepSeek, OpenAI, Qwen, or a local `ollama` / `vLLM` server just
  by editing `graph.json`.

## 1. Prerequisites

- Raspberry Pi 4B (8 GB) with a 64-bit Debian-based OS.
- Rust stable (see `rust-toolchain.toml`) and Python 3.
- One Xiaozhi ESP32 board already flashed with the official firmware.

## 2. Setup

```bash
./setup.sh          # installs libopus, websockets, Qwen deps, creates .env
```

Edit `.env`:

```dotenv
DEEPSEEK_API_KEY=sk-...
DASHSCOPE_API_KEY=sk-...
DASHSCOPE_WORKSPACE_ID=...
```

## 3. Run

```bash
./run.sh
```

The WebSocket server listens on `0.0.0.0:8888` by default. In the Xiaozhi
firmware configuration (OTA / websocket URL), set:

```
ws://<raspberry-pi-ip>:8888
```

Power the board, wait for the wake word, and talk. Speak again to barge in and
interrupt the assistant.

## 4. Configuring the LLM

The LLM Node is a generic OpenAI-compatible adapter. Edit `graph.json` → `llm`:

| Provider | `endpoint` | `model` | `api_key_env` |
| --- | --- | --- | --- |
| DeepSeek (default) | `https://api.deepseek.com/v1` | `deepseek-chat` | `DEEPSEEK_API_KEY` |
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` | `OPENAI_API_KEY` |
| Qwen (Bailian) | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `qwen-flash` | `DASHSCOPE_API_KEY` |
| Ollama (local CPU) | `http://127.0.0.1:11434/v1` | `qwen2.5:7b` | *(empty)* |

For Ollama, leave `api_key_env` empty and start `ollama serve` on the Pi. A 4B
with 8 GB can run small quantized models, but expect slower responses than a
cloud endpoint.

### Using the Pi coding Agent instead of a plain LLM

The flagship demo already ships a `pi.agent` TypeScript Node that wraps the Pi
coding agent. You can replace `builtin.llm_openai_compatible` with `pi.agent`
(keep its `system_prompt` voice-first and keep `builtin.speech_formatter`
downstream so Markdown never reaches TTS). The agent adds workspace file tools
and web search at the cost of higher latency and heavier first-token time.

## 5. Local models on the Pi (optional)

The transport and VAD are already CPU-only and lightweight. For full local
inference:

- **VAD**: use `builtin.audio_vad` (energy-based, no model).
- **ASR**: add a `faster-whisper` or `whisper.cpp` provider Node (future work).
- **TTS**: add a `piper` provider Node (very fast on Pi).
- **LLM**: point `builtin.llm_openai_compatible` at a local `ollama` endpoint.

## 6. Automated full-duplex test

`tests/test_full_duplex.py` reproduces a complete three-turn conversation
without flashing any firmware. It synthesizes the user voice with Qwen TTS,
streams it into the server as Opus (exactly like the device microphone), and
verifies the greeting, a joke, and a weather barge-in.

```bash
cd examples/xiaozhi-agent
# 1. Generate the user-voice fixtures (needs DASHSCOPE_API_KEY).
DASHSCOPE_API_KEY=... python3 tests/make_fixtures.py

# 2. Run the full-duplex case (needs both DashScope values).
DASHSCOPE_API_KEY=... DASHSCOPE_WORKSPACE_ID=... \
    python3 -m unittest tests.test_full_duplex -v
```

The test starts `muxiva serve` itself, connects a simulated device, and checks
the `stt` / `tts` text plus assistant audio for each turn. Credentials are read
from the environment or `.env` and never written to disk.

## 7. Free quota and cost (Alibaba Cloud Model Studio)

The default graph already uses the cheapest **new realtime-protocol** models,
and every one of them has free quota on new accounts (verified against the
Model Studio quota API):

| Stage | Model | Price after free quota | Free quota |
| --- | --- | --- | --- |
| LLM | `qwen-flash` | ¥0.15 / 1M input tokens, ¥1.5 / 1M output | **5,000,000 tokens / month** + 15,000 requests |
| ASR | `qwen3-asr-flash-realtime` | ¥0.00033 / second | **20 sessions / day** (one server run = one session) |
| TTS | `qwen3-tts-flash-realtime` | ¥1 / 10,000 chars | **3 sessions / day** (one reply = one session) |

Notes:

- One TTS WebSocket session synthesizes many sentences, so one assistant reply
  is one session. A barge-in closes the session (the model cannot cleanly
  resume after an in-session cancel), so an interrupted turn costs two.
- Beyond the free quota, costs are negligible for voice use: a typical 50-char
  reply is ¥0.005, so even a heavy 30-turn day is well under ¥1.
- Cheaper list prices (e.g. `paraformer-realtime-v2` ASR at ¥0.00024/s,
  `qwen3-tts-flash` at ¥0.8/10k chars) use the legacy realtime protocol or
  have no free quota; they were tested and are **not** drop-in replacements.
- The only fully-free path is local models (Piper TTS + Faster-Whisper ASR +
  a local LLM), at the cost of latency and quality on a Pi 4B.

## 8. Troubleshooting

- `libopus is not installed` → `sudo apt-get install -y libopus0`.
- `websockets is not installed` → `pip install websockets`.
- Device never connects → check the Pi firewall for TCP `8888` and that the
  firmware websocket URL uses `ws://` (not `wss://`) on the LAN.
- No assistant audio → confirm DashScope credentials and that `qwen-tts` output
  (24 kHz) is resampled to 16 kHz by `tts-resampler`.
- Responses are slow → the default is fully remote; local LLM/ASR/TTS will be
  slower on a Pi 4B.
