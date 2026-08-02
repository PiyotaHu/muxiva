# Qwen Audio Realtime

Runs speech understanding, turn detection, reasoning, and speech generation in one persistent
realtime session. Use it for the lowest-latency speech-to-speech graph.

| Property | Value |
| --- | --- |
| Node type | `provider.qwen.audio_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.to.speech.realtime` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 16 kHz, mono, streaming |
| `audio_out` | Output Audio | PCM S16LE, 24 kHz, mono, streaming |
| `text_out` | Output Text | User and assistant transcript deltas |

## Configuration

`model` defaults to `qwen-audio-3.0-realtime-flash`; `voice` selects the synthesized voice;
`instructions` defines assistant behavior; `turn_detection` accepts `server_vad` or `smart_turn`.
The flagship demo recommends `server_vad` with a default threshold of `0.35` and a `600ms`
silence boundary because it is deterministic and easier to diagnose.

On interruption, Voxa cancels Provider generation, propagates a `voxa.runtime.interrupt` Signal,
and clears queued PCM in the Agora Sink. Voice Room renders `YOU ARE SPEAKING` and
`BARGE-IN · INTERRUPTING AGENT`; `[VOXA][AGORA][audio.cancelled]` reports the exact number of
bytes removed from pending playback.
