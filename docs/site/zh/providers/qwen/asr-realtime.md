# Qwen Streaming ASR

把流式语音转换为临时和最终 Transcript，用于链路清晰、每层可替换的级联 Graph。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.asr_realtime` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `speech.asr.streaming` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_in` | 输入 Audio | PCM S16LE、16 kHz、单声道、流式 |
| `text_out` | 输出 Text | 最终 Transcript |
| `client_event_out` | 输出 Event | 面向客户端的最终转写事件 |

## 配置

`model` 默认是 `qwen3-asr-flash-realtime`；`language` 默认是 `zh`；`vad_threshold` 和
`silence_duration_ms` 用于调整一句话结束判定。需要配置共享的 `dashscope` Connection。
