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
| `event_out` | Output Event | Speech state, response completion, barge-in, and transcript failure |
| `signal_out` | Output Signal | Barge-in/cancellation control routed by explicit Edges |

## Configuration

`model` defaults to `qwen-audio-3.0-realtime-flash`; `voice` selects the synthesized voice;
`instructions` defines assistant behavior; `turn_detection` accepts `server_vad` or `smart_turn`.
The flagship demo uses Qwen Realtime's internal `server_vad`, preserving the boundary that one
Realtime Node owns VAD, ASR, LLM, and TTS.
`input_chunk_ms` defaults to
`100`: the Node combines
Agora's 10ms PCM Frames into the 100ms/3200-byte WebSocket chunks recommended by Model Studio
while continuing to poll server events on every Runtime callback.
Before sending any audio, the Node completes the strict
`session.created → session.update → session.updated` handshake. A timeout or rejected
configuration fails Runtime startup instead of leaving a false-running session whose audio count
grows while the model never responds.

On interruption, the Qwen Node cancels its own generation and emits a
`muxiva.voice.speech.started` Signal; the Agora Audio Sink receives it and clears queued PCM.
The algorithm Node has no client-specific Port. The project-local `voice_room.event_encoder`
consumes `transcript_preview_out`, `transcript_out`, `response_text_out`, and `event_out`, then
builds the Voice Room protocol. NotificationBus remains local observability. Voice Room renders `YOU ARE SPEAKING` and
`BARGE-IN · INTERRUPTING AGENT`; `[MUXIVA][AGORA][audio.cancelled]` reports the exact number of
bytes removed from pending playback.

If RTC Frames keep increasing but there is no response, inspect `input.audio_peak_pcm16` and
`input.audio_mean_abs_pcm16` on the Qwen Node in Observe. Values that remain near zero
mean the browser is publishing silence or the wrong input device, rather than an ASR network
bottleneck. Voice Room also shows a live **MIC LEVEL** and emits a yellow diagnostic when it sees no
speech energy for five seconds.
