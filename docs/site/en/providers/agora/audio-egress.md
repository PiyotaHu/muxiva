# Agora Audio Egress

Publishes generated PCM Audio Frames to remote Agora subscribers.

| Property | Value |
| --- | --- |
| Node type | `agora.audio_sink` |
| Layer / kind | `transport` / `sink` |
| Capability | `rtc.audio.egress` |
| Language | C++ |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 48 kHz, mono, 20 ms, streaming |
| `signal_in` | Input Signal | Explicit graph-control edge |

Configure the shared `agora` Connection. On `voxa.voice.speech.started`, this Node clears its
pending PCM queue and advances a cancellation sequence watermark. Audio that was already queued
elsewhere in the Graph but arrives at or below that watermark is discarded. The behavior belongs
to the playback Node; Core does no voice-turn filtering.

```text
tts.audio_out -> agora-egress.audio_in
```

If a TTS Node emits another sample rate, place `builtin.audio_resampler` before this Node.
