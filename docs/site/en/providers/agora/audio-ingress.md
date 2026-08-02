# Agora Audio Ingress

Receives remote audio from an Agora channel and emits realtime PCM Frames.

| Property | Value |
| --- | --- |
| Node type | `provider.agora.audio_source` |
| Layer / kind | `transport` / `transform` |
| Capability | `rtc.audio.ingress` |
| Language | C++ |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `tick_in` | Input Event | Poll tick used to drain the bounded native receive queue |
| `audio_out` | Output Audio | PCM S16LE, 16 kHz, mono, 20 ms, streaming |

The Node is tick-driven so the native SDK callback never executes Graph work directly. Configure
the shared `agora` Connection; the Node has no per-instance configuration in v1.

```text
interval-tick.tick_out -> agora-ingress.tick_in
agora-ingress.audio_out -> asr.audio_in
```

Stop or abort closes admission before leaving the channel. Late native callbacks are discarded.
