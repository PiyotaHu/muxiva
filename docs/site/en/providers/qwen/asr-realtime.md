# Qwen Streaming ASR

Converts streaming speech to partial and final transcripts for an inspectable cascade graph.

| Property | Value |
| --- | --- |
| Node type | `provider.qwen.asr_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.asr.streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 16 kHz, mono, streaming |
| `text_out` | Output Text | Partial and final transcripts |

## Configuration

`model` defaults to `qwen3-asr-flash-realtime`; `language` defaults to `zh`; `vad_threshold` and
`silence_duration_ms` tune utterance completion. Configure the shared `dashscope` Connection.
