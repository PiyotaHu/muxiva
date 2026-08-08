# Qwen Streaming ASR

使用阿里云 Qwen Server VAD 检测说话起止，同时把流式语音转换为预览和最终 Transcript。
它是 Demo 2 的打断源：`speech.started` 会从 `signal_out` 进入显式控制 Edge。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.asr_realtime` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `speech.vad_asr.streaming` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_in` | 输入 Audio | PCM S16LE、16 kHz、单声道、流式 |
| `speech_out` | 输出 Event | Server VAD 的 `speech.started` / `speech.stopped` |
| `signal_out` | 输出 Signal | `muxiva.voice.speech.started`，用于 Barge-in |
| `text_out` | 输出 Text | 最终 Transcript |
| `client_event_out` | 输出 Event | 说话状态、转写预览/完成/失败事件 |

## 配置

`model` 默认是 `qwen3-asr-flash-realtime`；`language` 默认是 `zh`；`vad_threshold` 和
`silence_duration_ms` 用于调整一句话结束判定。需要配置共享的 `dashscope` Connection。

协议与模型范围以[阿里云 Qwen 实时语音识别文档](https://help.aliyun.com/zh/model-studio/real-time-speech-recognition-user-guide)为准。
