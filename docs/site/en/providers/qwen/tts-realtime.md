# Qwen Streaming TTS

Synthesizes incremental response text into streaming speech.

| Property | Value |
| --- | --- |
| Node type | `qwen.tts_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.tts.streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `text_in` | Input Text | Incremental synthesis text |
| `audio_out` | Output Audio | PCM S16LE, 24 kHz, mono, streaming |

## Configuration

`model` defaults to `qwen3-tts-flash-realtime`; `voice` defaults to `Cherry`; `language_type`
defaults to `Auto`. Add an Audio Resample Node when the downstream transport requires 16 kHz.
