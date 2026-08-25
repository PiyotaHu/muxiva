# Muxiva 内置 Node

Builtin 是编译进 Muxiva 的厂商无关 Factory。即使它们共享 Rust Runtime 二进制，也会按照
真实能力进行分类。

| Node Type | 层级 | Capability | 契约 |
| --- | --- | --- | --- |
| `builtin.audio_resampler` | Media | `audio.resample` | PCM S16LE Audio 输入，指定采样率 Audio 输出 |
| `builtin.audio_vad` | Algorithm | `speech.vad` | PCM Audio 输入，语音活动 Event 输出 |
| `builtin.speech_formatter` | Algorithm | `text.speech_format` | 流式 Markdown Text 与取消 Signal 输入，适合 TTS 的纯文本输出 |
| `builtin.voice_turn_context` | Control | `conversation.turn_context` | Transcript 与语音 Event 输入，轮次上下文 Text 输出 |
| `builtin.voice_turn_controller` | Control | `conversation.turn_control` | 过滤无意义转写、批准新轮次并唯一发出标准取消 Signal |
| `builtin.interval_tick` | Control | `clock.interval` | 周期 Event 输出 |
| `builtin.text_source` | Utility | `text.source` | 配置的 UTF-8 Text 输出 |
| `builtin.uppercase` | Utility | `text.uppercase` | UTF-8 Text 输入，大写 Text 输出 |
| `builtin.text_sink` | Utility | `text.collect` | UTF-8 Text 输入 |
| `builtin.stdout_text_sink` | Utility | `observability.stdout` | UTF-8 Text 输入和带品牌的 stdout 日志 |

`builtin.demo.*` 是测试使用的确定性架构预览，分类为 `utility / demo.voice`。它们不是生产级
麦克风、ASR、LLM、TTS 或扬声器 Node。

在 Studio 中选择任意 Builtin 即可检查配置与 Port Schema。媒体转换必须显式表达：采样率
不兼容时应连接 `builtin.audio_resampler`，Edge 不会偷偷转换格式。
这个通用 Node 通过 `input` 与 `output` 配置对象声明 `sample_format`、`sample_rate_hz`
和 `channels`，因此输入 16 kHz 与输出 48 kHz 不再是两种专用 Node Type。

`builtin.speech_formatter` 让 Agent 原始 Markdown 继续分叉到富文本聊天界面，只把派生的
纯文本送入 TTS。它会删除强调符号和裸 URL、保留链接标题，并把跨 Frame 的代码围栏和
Markdown 表格替换为可配置的播报提示。它还通过
`minimum_chunk_characters` 和 `maximum_chunk_characters` 负责 TTS 断句提交；Agent 本身只
输出语义文本增量，不包含语音策略。Agent 终止 Event 会冲刷最后一段。
`suppressed_parenthetical_terms` 可按产品配置不应朗读的括号内呈现词；Builtin 不内置
任何角色或设备词，小智的动作词表只存在于小智 Graph 配置中。
解析器会跨 Text Frame 保存代码围栏和表格状态，因此不会把半截 Markdown 控制符送进 TTS。
打断 Signal 或新的 Sequence 会重置这些状态，未闭合的旧 Markdown 不会导致下一轮静音。

`builtin.voice_turn_controller` 是级联语音图唯一的中断裁决点。VAD 的
`speech.started/stopped` 只是观察事件；最终 Transcript 通过策略校验后，控制器才发出
`muxiva.turn.cancelled`，同时输出带相同 Sequence 的 Prompt。`嗯`、`啊`、咳嗽、最短
长度与短指令白名单都通过配置调整。设备的强制停止先发
`muxiva.turn.interrupt.requested`，也必须经过控制器再扇出到 Agent、TTS 和播放节点。
