# Agora Audio Egress

Publishes generated PCM Audio Frames to remote Agora subscribers.

| Property | Value |
| --- | --- |
| Node type | `provider.agora.audio_sink` |
| Layer / kind | `transport` / `sink` |
| Capability | `rtc.audio.egress` |
| Language | C++ |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 16 kHz, mono, 20 ms, streaming |

Configure the shared `agora` Connection. Voxa applies stale-turn filtering before the Sink, so
audio from an interrupted turn cannot be published after a newer turn begins.

```text
tts.audio_out -> agora-egress.audio_in
```

If a TTS Provider emits another sample rate, place `builtin.audio_resample` before this Node.
