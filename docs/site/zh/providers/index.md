# Provider 与 Node 目录

Voxa 将每个 Node 的两个属性明确分开：

- **架构层级**：`transport`、`algorithm`、`media`、`control` 或 `utility`。
- **图中角色**：`source`、`transform` 或 `sink`。

Provider 的厂商信息、凭据、SDK 兼容性、License 和文档只在 `voxa.provider.json` 声明一次；
每个 `voxa.node.json` 声明稳定的 Capability 和精确 Port Schema。

| 层级 | Provider | 能力 |
| --- | --- | --- |
| 传输层 | [Agora RTC](agora/index.md) | RTC 音频输入与输出 |
| 算法层 | [阿里云 Qwen](qwen/index.md) | 实时语音、ASR、LLM 与 TTS |
| 媒体、控制、工具 | [Voxa 内置 Node](builtin.md) | 重采样、VAD、轮次上下文、时钟与诊断 |

Studio 会递归发现配置的 Provider Root。可以在 Palette 中按层级过滤，也可以按厂商、
Capability、Tag 或 Node Type 搜索。
