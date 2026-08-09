# Qwen Streaming LLM

Consumes transcript or context text and emits sentence-sized assistant response deltas.

| Property | Value |
| --- | --- |
| Node type | `qwen.llm_stream` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `language.generation.cancellable_streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `text_in` | Input Text | Prompt or turn context, streaming |
| `signal_in` | Input Signal | Cancels the active SSE response and clears stale results |
| `text_out` | Output Text | Assistant response deltas, streaming |
| `event_out` | Output Event | Response completion state |

## Configuration

`model` defaults to `qwen-flash`; `system_prompt` defines behavior; `temperature` defaults to
`0.6`. Sentence-sized output lets TTS start before the full response completes. HTTP SSE runs on
a background worker; callbacks only start requests or drain a bounded queue, so
`on_signal` can close an in-flight response immediately. The Node requests Runtime-managed
internal wakeups through its Context, so it exposes no Clock Node or `tick_in` Port.
`max_results_per_wakeup` defaults to `32`.
The Node does not know about Voice Room or any client protocol. A project Node maps `text_out` and
`event_out` when a remote UI needs them.

The protocol follows Alibaba Cloud's [OpenAI-compatible Qwen streaming output](https://help.aliyun.com/en/model-studio/stream).
