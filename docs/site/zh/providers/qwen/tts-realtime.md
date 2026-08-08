# Qwen Streaming TTS

把增量响应文本合成为流式语音。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.tts_realtime` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `speech.tts.cancellable_streaming` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `text_in` | 输入 Text | 增量合成文本 |
| `tick_in` | 输入 Event | 通用 Runtime Tick，用于排空后台 PCM |
| `signal_in` | 输入 Signal | 用户开口时关闭当前 TTS WebSocket，清空待合成文本与 PCM |
| `audio_out` | 输出 Audio | PCM S16LE、24 kHz、单声道、流式 |

## 配置

`model` 默认是 `qwen3-tts-flash-realtime`；`voice` 默认是 `Cherry`；`language_type` 默认是
`Auto`。后台 Worker 复用同一 TTS Session 合成连续句子，减少句间重连和跳播；发生 Signal
时关闭 Session 并推进 generation，晚到 PCM 会被丢弃。`max_results_per_tick` 默认是 `64`。
Demo 2 使用 Audio Resample Node 把 24 kHz 输出转换成 Agora 的 48 kHz PCM。

协议与模型范围见[阿里云 Qwen 实时语音合成文档](https://help.aliyun.com/zh/model-studio/interactive-process-of-qwen-tts-realtime-synthesis)。
