# Voxa 内置 Node

Builtin 是编译进 Voxa 的厂商无关 Factory。即使它们共享 Rust Runtime 二进制，也会按照
真实能力进行分类。

| Node Type | 层级 | Capability | 契约 |
| --- | --- | --- | --- |
| `builtin.audio_resample` | Media | `audio.resample` | PCM S16LE Audio 输入，指定采样率 Audio 输出 |
| `builtin.audio_vad` | Algorithm | `speech.vad` | PCM Audio 输入，语音活动 Event 输出 |
| `builtin.voice_turn_context` | Control | `conversation.turn_context` | Transcript 与语音 Event 输入，轮次上下文 Text 输出 |
| `builtin.interval_tick` | Control | `clock.interval` | 周期 Event 输出 |
| `builtin.text_source` | Utility | `text.source` | 配置的 UTF-8 Text 输出 |
| `builtin.uppercase` | Utility | `text.uppercase` | UTF-8 Text 输入，大写 Text 输出 |
| `builtin.text_sink` | Utility | `text.collect` | UTF-8 Text 输入 |
| `builtin.stdout_text_sink` | Utility | `observability.stdout` | UTF-8 Text 输入和带品牌的 stdout 日志 |

`builtin.demo.*` 是测试使用的确定性架构预览，分类为 `utility / demo.voice`。它们不是生产级
麦克风、ASR、LLM、TTS 或扬声器 Provider。

在 Studio 中选择任意 Builtin 即可检查配置与 Port Schema。媒体转换必须显式表达：采样率
不兼容时应连接 `builtin.audio_resample`，Edge 不会偷偷转换格式。
