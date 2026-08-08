# Qwen Streaming ASR

Uses Alibaba Cloud Qwen Server VAD to detect speech boundaries while producing preview and final
transcripts. It is Demo 2's interruption source: `speech.started` leaves through `signal_out` and
enters explicit control Edges.

| Property | Value |
| --- | --- |
| Node type | `qwen.asr_realtime` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `speech.vad_asr.streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `audio_in` | Input Audio | PCM S16LE, 16 kHz, mono, streaming |
| `speech_out` | Output Event | Server-VAD `speech.started` / `speech.stopped` |
| `signal_out` | Output Signal | `muxiva.voice.speech.started` for barge-in |
| `text_out` | Output Text | Final transcript |
| `client_event_out` | Output Event | Speech state plus transcript preview/completion/failure |

## Configuration

`model` defaults to `qwen3-asr-flash-realtime`; `language` defaults to `zh`; `vad_threshold` and
`silence_duration_ms` tune utterance completion. Configure the shared `dashscope` Connection.

See Alibaba Cloud's [Qwen real-time speech recognition guide](https://help.aliyun.com/en/model-studio/real-time-speech-recognition-user-guide) for the current protocol and model scope.
