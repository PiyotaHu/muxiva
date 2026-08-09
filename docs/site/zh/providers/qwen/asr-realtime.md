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
| `transcript_preview_out` | 输出 Text | 临时转写 |
| `text_out` | 输出 Text | Server VAD 已结束该轮后提交的最终 Transcript |
| `event_out` | 输出 Event | 转写失败状态 |

## 配置

`model` 默认是 `qwen3-asr-flash-realtime`，`language` 默认是 `zh`。Demo 2 默认把
`vad_threshold` 设为 `0.45`：数值越低越容易触发，越高越能过滤低能量声音；
`silence_duration_ms` 用于调整一句话结束判定。需要调节时，在 Studio 画布选择
`qwen-vad-asr`，修改 **Configuration** 中的 `vad_threshold`，再点击 **Validate** 和
**Save graph**。需要配置共享的 `dashscope` Connection。
该 Node 只输出与客户端无关的语义 Frame。Demo 2 会把 Text/Event 分叉给 Graph 内部处理
Node 和项目级 Voice Room 协议 Node。即使厂商事件乱序，Node 也会等待 `speech.stopped`
后才提交 `text_out`，并丢弃 Final 之后迟到的 Preview，因此不需要额外 Turn Context Node。

协议与模型范围以[阿里云 Qwen 实时语音识别文档](https://help.aliyun.com/zh/model-studio/real-time-speech-recognition-user-guide)为准。
