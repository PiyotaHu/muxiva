# 官方与自定义 Node

Agora、Qwen 等集成都是普通 Node。Core 不链接厂商 SDK，也不理解 ASR、TTS、RTC 或对话
Turn；这些语义留在对应 Node 中。

官方语音 Node：

- `agora.audio_source`：C++ RTC 音频 Source；内部自调度，不需要外部 Clock Node。
- `builtin.voice_turn_controller`：厂商无关的轮次准入和唯一取消裁决点。
- `agora.audio_sink`：C++ RTC 音频 Sink；收到标准 `muxiva.turn.cancelled` 后清空播放队列。
- `qwen.audio_realtime`：Python Speech-to-Speech Node；负责 VAD、ASR、推理、TTS 和取消迟到响应。
- `qwen.asr_realtime`：Qwen Server VAD + ASR；只输出活动观察和 Transcript。
- `qwen.llm_stream`、`qwen.tts_realtime`：可替换的后台 Node，通过 Tick 排空结果，并在
  `muxiva.turn.cancelled` 时取消进行中的工作。
- `pi.agent`：Demo 2 的 TypeScript 适配 Node；它通过薄适配器加载独立发布的
  [Pi 编码 Agent](nodes/pi-agent.md)，负责会话、Tool Call 与受限文件编码能力，并遵守
  [Agent 集成契约](nodes/agent-integration.md)。

项目 Node 放在 `.muxiva/nodes/`。每个目录包含 `muxiva.node.json` 和语言入口文件；Studio
可以查看源码、注册并拖入 Graph。Connection 字段通过 Manifest 声明，真实值写入被
Git 忽略的 `.env`。
