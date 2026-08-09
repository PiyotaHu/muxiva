# Qwen Streaming TTS

Synthesizes incremental response text into streaming speech.

| Property | Value |
| --- | --- |
| Node type | `qwen.tts_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.tts.cancellable_streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `text_in` | Input Text | Incremental synthesis text |
| `signal_in` | Input Signal | Closes the WebSocket, clears queues, and rejects old-turn Text |
| `audio_out` | Output Audio | PCM S16LE, 24 kHz, mono, streaming |

## Configuration

`model` defaults to `qwen3-tts-flash-realtime`; `voice` defaults to `Cherry`; `language_type`
defaults to `Auto`. The worker reuses one TTS session across sentence chunks to avoid reconnect
gaps. A Signal closes that session, advances its generation, and rejects late PCM.
It also records the cancellation sequence and rejects stale Text. The worker requests
Runtime-managed internal wakeups through its Context, so no Clock Node is required.
`max_results_per_wakeup` defaults to `64`. Demo 2 resamples 24 kHz output to Agora's 48 kHz PCM.

See Alibaba Cloud's [Qwen real-time speech synthesis protocol](https://help.aliyun.com/en/model-studio/interactive-process-of-qwen-tts-realtime-synthesis).
