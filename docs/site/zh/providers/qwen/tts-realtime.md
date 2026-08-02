# Qwen Streaming TTS

把增量响应文本合成为流式语音。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.tts_realtime` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `speech.tts.streaming` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `text_in` | 输入 Text | 增量合成文本 |
| `audio_out` | 输出 Audio | PCM S16LE、24 kHz、单声道、流式 |

## 配置

`model` 默认是 `qwen3-tts-flash-realtime`；`voice` 默认是 `Cherry`；`language_type` 默认是
`Auto`。下游 Transport 如果要求 16 kHz，需要在中间加入 Audio Resample Node。
