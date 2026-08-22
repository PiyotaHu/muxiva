# Xiaozhi ESP32 voice interaction

Muxiva supports the open-source [Xiaozhi ESP32](https://github.com/78/xiaozhi-esp32)
voice assistant device as a first-class client. The ESP32 board connects to a
Muxiva voice graph over its native WebSocket + Opus protocol, giving the device
a full **VAD + ASR + LLM + TTS** pipeline without writing any firmware.

- Transport provider: `providers/transport/xiaozhi` (Python)
- Category: `transport`
- Device protocol: Xiaozhi WebSocket `v1` (JSON control + Opus audio)
- Flagship example: [`examples/xiaozhi-agent`](https://github.com/PiyotaHu/muxiva/tree/main/examples/xiaozhi-agent)
- Credentials: Alibaba Cloud Model Studio API Key + Workspace ID
- Billing: check current Model Studio pricing and quota documentation; this page does not pin time-sensitive prices

## Device protocol

The Xiaozhi firmware speaks a small JSON + Opus protocol over one WebSocket:

| Direction | Message | Meaning |
| --- | --- | --- |
| device → server | `{"type":"hello"}` | handshake; server replies with negotiated Opus audio params |
| device → server | binary Opus packets | microphone audio (60 ms frames) |
| device → server | `{"type":"abort"}` | user pressed the interrupt button |
| device → server | `{"type":"listen",...}` | device listen-state transition |
| device → server | `{"type":"ping"}` | keep-alive; server replies `pong` |
| server → device | `{"type":"hello",...}` | negotiated `audio_params` plus `session_id` |
| server → device | binary Opus packets | assistant speech |
| server → device | `{"type":"stt","text":...}` | user transcript rendered on the device display |
| server → device | `{"type":"tts","state":...}` | assistant speaking state / answer text |

The device display therefore shows the ASR question (`stt`), the LLM answer
(`tts sentence_start`), and the speaking / interruption states (`tts start` /
`stop`) in real time.

## Architecture

Three Node packs make up the transport provider, following the same layer as
the Agora RTC provider:

- **`xiaozhi.audio_source`** (Source): hosts the WebSocket server, decodes Opus
  to 16 kHz PCM, forwards device controls, buffers outbound playback, and paces
  Opus packets in real time.
- **`xiaozhi.audio_sink`** (Sink): encodes TTS PCM back to Opus and streams it
  to the device.
- **`xiaozhi.event_encoder`** (Sink): maps transcripts, assistant text, TTS
  lifecycle, and transport-neutral emotion Events into device protocol messages.

Every Muxiva Python Node runs in its own process, so the Source Node owns a
small in-process gateway and the Sink / Event Encoder Nodes connect to it over a
loopback JSON-lines control socket. Only PCM Frames and control Signals/Events
cross the runtime boundary; Opus and the WebSocket protocol stay inside the
transport provider.

## Example graph

```text
ESP32 (Opus over WebSocket)
        │  ws://<server-ip>:8888
        ▼
xiaozhi.audio_source ──► qwen.asr_realtime ──► pi.agent
   (Opus gateway)          (server VAD + ASR)    (routes + tools + model)
        ▲                                                     │
        │                                                     ▼
xiaozhi.audio_sink ◄── builtin.audio_resampler ◄── qwen.tts_realtime
        ▲                        │                     ▲
        └────────────────────────┴── builtin.speech_formatter
```

The graph supports full-duplex conversation: the user can interrupt the
assistant mid-response (barge-in), the server cancels the active TTS/Agent work
and immediately answers the new turn.

## Quick start (Raspberry Pi 4B)

```bash
cd examples/xiaozhi-agent
./setup.sh                     # installs libopus, websockets, Qwen deps, creates .env
./run.sh                       # starts `muxiva serve`; WebSocket on 0.0.0.0:8888
```

Point the firmware WebSocket URL at `ws://<raspberry-pi-ip>:8888` and talk.

## Automated full-duplex test

`examples/xiaozhi-agent/tests/test_full_duplex.py` reproduces a three-turn
conversation (greeting, joke, weather barge-in) without any hardware. It
synthesizes the user voice with Qwen TTS, streams it as Opus exactly like the
device microphone, and verifies the `stt` / `tts` display sequence plus the
barge-in signal. See the [example README](https://github.com/PiyotaHu/muxiva/tree/main/examples/xiaozhi-agent) for the full commands.
