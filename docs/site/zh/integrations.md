# 官方与自定义 Node

Agora、Qwen 等集成都是普通 Node。Core 不链接厂商 SDK，也不理解 ASR、TTS、RTC 或对话
Turn；这些语义留在对应 Node 中。

官方语音 Node：

- `agora.audio_source`：C++ RTC 音频 Source；内部自调度，不需要外部 Clock Node。
- `agora.audio_sink`：C++ RTC 音频 Sink；收到 `muxiva.voice.speech.started` Signal 后清空播放队列。
- `qwen.audio_realtime`：Python Speech-to-Speech Node；负责 VAD、ASR、推理、TTS 和取消迟到响应。
- `qwen.asr_realtime`：Qwen Server VAD + ASR，也是级联打断 Signal 的来源。
- `qwen.llm_stream`、`qwen.tts_realtime`：可替换的后台 Node，通过 Tick 排空结果，并在
  `muxiva.voice.speech.started` 时关闭进行中的厂商连接。
- `pi.agent`：Demo 2 的 TypeScript 适配 Node；它通过薄适配器加载独立发布的
  [Pi 编码 Agent](nodes/pi-agent.md)，负责会话、Tool Call 与受限文件编码能力，并遵守
  [Agent 集成契约](nodes/agent-integration.md)。

项目 Node 放在 `.muxiva/nodes/`。每个目录包含 `muxiva.node.json` 和语言入口文件；Studio
可以查看源码、注册并拖入 Graph。Connection 字段通过 Manifest 声明，真实值写入被
Git 忽略的 `.env`。
