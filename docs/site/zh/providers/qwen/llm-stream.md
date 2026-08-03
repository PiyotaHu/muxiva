# Qwen Streaming LLM

接收 Transcript 或上下文文本，输出按句切分的助手响应增量。

| 属性 | 值 |
| --- | --- |
| Node Type | `qwen.llm_stream` |
| 层级 / 角色 | `algorithm` / `transform` |
| Capability | `language.generation.streaming` |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `text_in` | 输入 Text | Prompt 或轮次上下文，流式 |
| `text_out` | 输出 Text | 助手响应增量，流式 |
| `client_event_out` | 输出 Event | 回答增量与完成客户端事件 |

## 配置

`model` 默认是 `qwen-flash`；`system_prompt` 定义助手行为；`temperature` 默认是 `0.6`。
按句输出使 TTS 不必等待完整回答即可开始合成。当前 HTTP Stream 是同步实现：打断 Gate
会立即丢弃旧文本，但已发出的厂商请求仍会运行结束，这个 Node 才能接收下一份输入。
因此级联 Graph 不能宣称具备模型侧硬取消。
