# Xiaozhi Provider

The Xiaozhi provider is implemented in Python under `python`. It adapts the
[Xiaozhi ESP32](https://github.com/78/xiaozhi-esp32) WebSocket device protocol
to Muxiva typed Frames:

- `xiaozhi_audio_source`: a long-lived Source Node that hosts the WebSocket
  server, decodes Opus ingress to 16 kHz PCM, and forwards client control
  (`hello` / `abort` / `listen`) as Events and a barge-in Signal.
- `xiaozhi_audio_sink`: a Sink Node that receives TTS PCM and sends Opus packets
  back to the device through the shared gateway.
- `xiaozhi_event_encoder`: a Sink Node that maps graph transcripts and response
  text to the `stt` / `tts` JSON messages rendered on the device screen.

All three Nodes share one gateway that lives inside the Source Node process.
The Sink and Event Encoder connect to it over a loopback JSON-lines control
socket, so no state crosses process boundaries through the graph runtime.

## Dependencies

```bash
sudo apt-get install -y libopus0 libopus-dev
python3 -m pip install -r python/requirements.txt
```

The WebSocket server requires the `websockets` package. Opus is accessed
directly through `ctypes` against the system `libopus`, so no Python Opus
binding is required.

## Tests

```bash
python3 -m unittest discover -s providers/transport/xiaozhi/python/tests -v
```

`test_xiaozhi_gateway.py` simulates a Xiaozhi device end to end: it opens a
WebSocket connection, performs the `hello` handshake, streams Opus microphone
audio, sends `abort`, and asserts that the gateway decodes ingress PCM and
streams Opus plus `stt` / `tts` JSON back to the device.

## Raspberry Pi 4B notes

The adapter itself is lightweight (Opus codec plus one WebSocket). Run the
official firmware in *auto listen* mode (server VAD) and keep the device and
Pi on the same LAN. See `examples/xiaozhi-agent/README.md` for a complete setup.
