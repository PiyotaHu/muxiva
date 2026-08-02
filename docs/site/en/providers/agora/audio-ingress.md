# Agora Audio Ingress

Receives remote audio from an Agora channel and emits realtime PCM Frames.

| Property | Value |
| --- | --- |
| Node type | `agora.audio_source` |
| Layer / kind | `transport` / `source` |
| Capability | `rtc.audio.ingress` |
| Language | C++ |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_out` | Output Audio | PCM S16LE, 16 kHz, mono, 20 ms, streaming |

The native SDK callback only writes audio into a bounded queue. The Source calls
`ctx.schedule_next_tick(20ms)` to schedule its next internal drain, so callback threads never
execute Graph work and users do not see an `rtc-clock` or `tick_in` Port. Configure the shared
`agora` Connection; the Node has no per-instance configuration in v1.

```text
agora-ingress.audio_out -> asr.audio_in
```

Stop or abort closes admission before leaving the channel. Late native callbacks are discarded.

After upgrading from an earlier release, rerun `./examples/voice-agent/setup.sh`. The Source
Factory version changed from `1.0.0` to `1.1.0` to distinguish the old external-tick contract
from internal self-scheduling.
