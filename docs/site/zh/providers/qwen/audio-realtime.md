# Qwen Audio Realtime

在一个持久实时会话中完成语音理解、轮次检测、推理和语音生成，适合追求最低延迟的
Speech-to-Speech Graph。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.audio_realtime` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `speech.to.speech.realtime` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_in` | 输入 Audio | PCM S16LE、16 kHz、单声道、流式 |
| `audio_out` | 输出 Audio | PCM S16LE、24 kHz、单声道、流式 |
| `transcript_preview_out` | 输出 Text | 供 Graph 内部消费的用户临时转写 |
| `transcript_out` | 输出 Text | 用户最终转写 |
| `response_text_out` | 输出 Text | 助手回答增量 |
| `event_out` | 输出 Event | 说话状态、回答完成、打断和转写失败 |
| `signal_out` | 输出 Signal | 通过显式 Edge 路由的打断/取消控制 |

## 配置

`model` 默认是 `qwen-audio-3.0-realtime-flash`；`voice` 选择音色；`instructions` 定义助手行为；
`turn_detection` 支持 `server_vad` 或 `smart_turn`。门面 Demo 使用 Qwen Realtime 内部的
`server_vad`，保持“一个 Realtime Node 完成 VAD、ASR、LLM、TTS”的设计边界。
`input_chunk_ms` 默认是 `100`：Node 会把 Agora 的 10ms PCM Frame 聚合成百炼推荐的
100ms/3200-byte WebSocket 音频块，同时仍在每个 Runtime 回调中轮询服务端事件。
Node 在发送任何音频前会严格完成 `session.created → session.update → session.updated` 握手；
配置超时或被服务端拒绝时，Runtime 会直接给出协议错误，不会进入“音频在增长但模型没有响应”的假运行状态。

发生打断时，Qwen Node 会取消自己的生成、发出 `muxiva.voice.speech.started` Signal；Agora
Audio Sink 收到 Signal 后清空尚未播放的 PCM。算法 Node 不再包含客户端专用 Port；项目内的
`voice_room.event_encoder` 直接消费 `transcript_preview_out`、`transcript_out`、
`response_text_out` 与 `event_out`，再生成 Voice Room 协议。NotificationBus 仅用于本地观测。Voice Room 会显示 `YOU ARE SPEAKING` 和
`BARGE-IN · INTERRUPTING AGENT`；服务端日志中的 `[MUXIVA][AGORA][audio.cancelled]` 给出实际
清除的字节数。

如果 RTC Frame 持续增长但没有回答，先看 Observe 中 Qwen Node 的
`input.audio_peak_pcm16` 和 `input.audio_mean_abs_pcm16`：二者始终接近 0 说明浏览器发布的是
静音或选错了输入设备，并不是 ASR 网络堵塞。Voice Room 同时直接显示浏览器的 **MIC LEVEL**，
连续 5 秒没有检测到语音能量会给出黄色诊断提示。
