# Qwen Streaming ASR

Converts streaming speech to a final transcript for an inspectable cascade graph and emits a
versioned final event for remote clients.

| Property | Value |
| --- | --- |
| Node type | `qwen.asr_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.asr.streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 16 kHz, mono, streaming |
| `text_out` | Output Text | Final transcript |
| `client_event_out` | Output Event | Final transcript client event |

## Configuration

`model` defaults to `qwen3-asr-flash-realtime`; `language` defaults to `zh`; `vad_threshold` and
`silence_duration_ms` tune utterance completion. Configure the shared `dashscope` Connection.
