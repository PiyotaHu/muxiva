# Qwen Audio Realtime

Runs speech understanding, turn detection, reasoning, and speech generation in one persistent
realtime session. Use it for the lowest-latency speech-to-speech graph.

| Property | Value |
| --- | --- |
| Node type | `qwen.audio_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.to.speech.realtime` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 16 kHz, mono, streaming |
| `audio_out` | Output Audio | PCM S16LE, 24 kHz, mono, streaming |
| `transcript_preview_out` | Output Text | Partial user transcript for local Graph consumers |
| `transcript_out` | Output Text | Final user transcript |
| `response_text_out` | Output Text | Assistant response deltas |
| `client_event_out` | Output Event | Versioned transcript, response, and speech-state events |
| `signal_out` | Output Signal | Barge-in/cancellation control routed by explicit Edges |

## Configuration

`model` defaults to `qwen-audio-3.0-realtime-flash`; `voice` selects the synthesized voice;
`instructions` defines assistant behavior; `turn_detection` accepts `server_vad` or `smart_turn`.
The flagship demo recommends `server_vad` with a default threshold of `0.35` and a `1000ms`
silence boundary so natural pauses do not split one utterance into two turns.

On interruption, the Qwen Node cancels its own generation and emits a
`muxiva.voice.speech.started` Signal; the Agora Audio Sink receives it and clears queued PCM.
`client_event_out` is encoded and sent by Transport Nodes to a remote client; NotificationBus remains
local observability. Voice Room renders `YOU ARE SPEAKING` and
`BARGE-IN · INTERRUPTING AGENT`; `[MUXIVA][AGORA][audio.cancelled]` reports the exact number of
bytes removed from pending playback.
