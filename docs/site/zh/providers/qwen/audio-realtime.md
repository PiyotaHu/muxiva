# Qwen Audio Realtime

在一个持久实时会话中完成语音理解、轮次检测、推理和语音生成，适合追求最低延迟的
Speech-to-Speech Graph。

| 属性 | 值 |
| --- | --- |
| Node Type | `provider.qwen.audio_realtime` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `speech.to.speech.realtime` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_in` | 输入 Audio | PCM S16LE、16 kHz、单声道、流式 |
| `audio_out` | 输出 Audio | PCM S16LE、24 kHz、单声道、流式 |
| `text_out` | 输出 Text | 用户和助手的增量文本 |

## 配置

`model` 默认是 `qwen-audio-3.0-realtime-flash`；`voice` 选择音色；`instructions` 定义助手行为；
`turn_detection` 支持 `server_vad` 或 `smart_turn`。门面 Demo 推荐 `server_vad`，默认阈值为
`0.35`、静音结束时间为 `1000ms`；它能容纳自然短停顿，避免把一句话误切成两轮。

发生打断时，Voxa 会取消 Provider 生成、发出 `voxa.runtime.interrupt` Signal，并清空 Agora
Sink 中尚未播放的 PCM。Voice Room 会显示 `YOU ARE SPEAKING` 和
`BARGE-IN · INTERRUPTING AGENT`；服务端日志中的 `[VOXA][AGORA][audio.cancelled]` 给出实际
清除的字节数。
