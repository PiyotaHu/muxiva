# Xiaozhi Provider

The Xiaozhi provider adapts the [Xiaozhi ESP32](https://github.com/78/xiaozhi-esp32)
WebSocket device protocol to Muxiva. It lives entirely under
`providers/transport/xiaozhi` and is a transport provider, the same
architectural layer as the Agora RTC provider.

## Protocol surface

The device speaks a small JSON + Opus protocol:

| Direction | Message | Meaning |
| --- | --- | --- |
| device → server | `{"type":"hello"}` | handshake; server replies with negotiated audio params |
| device → server | binary Opus packets | microphone audio (60 ms frames) |
| device → server | `{"type":"abort"}` | user pressed interrupt |
| device → server | `{"type":"listen",...}` | device listen-state transition |
| device → server | `{"type":"ping"}` | keep-alive; server replies `pong` |
| server → device | `{"type":"hello",...}` | negotiated `audio_params` plus `session_id` |
| server → device | binary Opus packets | assistant speech |
| server → device | `{"type":"stt","text":...}` | user transcript for the device display |
| server → device | `{"type":"tts","state":...}` | assistant speaking state/display |

## Node packs

- `xiaozhi.audio_source` (`xiaozhi_audio_source`): **Source** Node that hosts the
  WebSocket server, decodes Opus to 16 kHz PCM, and forwards device control as
  Events plus a `muxiva.voice.speech.started` barge-in Signal.
- `xiaozhi.audio_sink` (`xiaozhi_audio_sink`): **Sink** Node that encodes TTS PCM
  to Opus and streams it back to the device.
- `xiaozhi.event_encoder` (`xiaozhi_event_encoder`): **Sink** Node that maps
  transcripts and assistant text into `stt`/`tts` device messages.

Because every Muxiva Python Node runs in its own process, the Source Node owns a
small in-process gateway. The Sink and Event Encoder Nodes connect to it over a
loopback JSON-lines control socket (`127.0.0.1:8889` by default). No Opus or
WebSocket object ever crosses the Muxiva runtime boundary; only PCM Frames and
control Signals/Events do.

!!! warning "Single-device prototype"
    The current Python gateway has one mutable active WebSocket and is only a
    development adapter. It is not the multi-user serving architecture. The
    production contract is [one accepted connection owning one isolated
    Session](../design/d13-connection-owned-sessions.md), with no Session Router,
    no global current socket, and no reverse endpoint HTTP control channel.

Optional endpoint-command forwarding is disabled by default. A deployment must
configure `device_command_topics`, `device_command_allowlist`, and
`device_command_message_type` on `xiaozhi.event_encoder`. Command meanings stay
outside Muxiva Core and are implemented by the endpoint/provider deployment.

## Dependencies

```bash
sudo apt-get install -y libopus0 libopus-dev
python3 -m pip install -r providers/transport/xiaozhi/python/requirements.txt
```

Opus is accessed through `ctypes` against the system `libopus`; `websockets`
provides the WebSocket server.

## Raspberry Pi 4B

The adapter itself is lightweight (Opus codec + one WebSocket connection) and
runs comfortably on a Pi 4B. Keep the device and the Pi on the same LAN and
point the firmware WebSocket URL at `ws://<pi-ip>:8888`. See
`examples/xiaozhi-agent/README.md` for the full voice pipeline.
