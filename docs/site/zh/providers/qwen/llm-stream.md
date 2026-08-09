# Qwen Streaming LLM

接收 Transcript 或上下文文本，输出按句切分的助手响应增量。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.llm_stream` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `language.generation.cancellable_streaming` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `text_in` | 输入 Text | Prompt 或轮次上下文，流式 |
| `tick_in` | 输入 Event | 通用 Runtime Tick，用于排空后台 SSE 结果 |
| `signal_in` | 输入 Signal | 用户开口时取消当前 SSE 请求并清空旧结果 |
| `text_out` | 输出 Text | 助手响应增量，流式 |
| `event_out` | 输出 Event | 回答完成状态 |

## 配置

`model` 默认是 `qwen-flash`；`system_prompt` 定义助手行为；`temperature` 默认是 `0.6`。
按句输出使 TTS 不必等待完整回答即可开始合成。HTTP SSE 在后台 Worker 中运行；Node
回调只负责启动请求或按 Tick 排空有界队列，因此 `on_signal` 能及时关闭进行中的响应，
而不是等待完整答案结束。`max_results_per_tick` 默认是 `32`。
该 Node 不知道 Voice Room 或任何客户端协议；远程 UI 需要这些信息时，由项目 Node 映射
`text_out` 和 `event_out`。

接口遵循[阿里云 Qwen OpenAI 兼容流式输出](https://help.aliyun.com/zh/model-studio/stream)协议。
