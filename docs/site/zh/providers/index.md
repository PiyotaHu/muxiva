# Node 目录

Voxa 将每个 Node 的两个属性明确分开：

- **架构层级**：`transport`、`algorithm`、`media`、`control` 或 `utility`。
- **图中角色**：`source`、`transform` 或 `sink`。

Voxa 的公开扩展单元只有 **Node**。每个 `voxa.node.json` 声明稳定的 Node Type、能力、
配置以及精确的 Port Schema；Connection 只负责让多个 Node 复用同一组本地凭据。

| 层级 | 官方 Node 集合 | 能力 |
| --- | --- | --- |
| 传输层 | [Agora RTC](agora/index.md) | RTC 音频与有序客户端消息输入/输出 |
| 算法层 | [阿里云 Qwen](qwen/index.md) | 实时语音、ASR、LLM 与 TTS |
| 媒体、控制、工具 | [Voxa 内置 Node](builtin.md) | 重采样、VAD、上下文与诊断 |

Studio 会递归发现官方 Node 和项目 `.voxa/nodes/` 下的自定义 Node。可以在 Palette 中按
层级过滤，也可以按 Capability、Tag 或 Node Type 搜索。
