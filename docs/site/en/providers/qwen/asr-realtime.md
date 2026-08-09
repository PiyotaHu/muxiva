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
| `transcript_preview_out` | Output Text | Partial transcript |
| `text_out` | Output Text | Final transcript committed after Server VAD closes the turn |
| `event_out` | Output Event | Transcript failure state |

## Configuration

`model` defaults to `qwen3-asr-flash-realtime`; `language` defaults to `zh`; `vad_threshold` and
`silence_duration_ms` tune utterance completion. Configure the shared `dashscope` Connection.
The Node emits provider-neutral semantic Frames only. Demo 2 fans its Text and Event outputs into
both Graph processing Nodes and the project-local Voice Room protocol Node. If vendor events are
reordered, the Node waits for `speech.stopped` before committing `text_out` and drops previews that
arrive after Final, so the Graph needs no separate Turn Context Node.

See Alibaba Cloud's [Qwen real-time speech recognition guide](https://help.aliyun.com/en/model-studio/real-time-speech-recognition-user-guide) for the current protocol and model scope.
