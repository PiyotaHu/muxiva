# Qwen Streaming LLM

Consumes transcript or context text and emits sentence-sized assistant response deltas.

| Property | Value |
| --- | --- |
| Node type | `provider.qwen.llm_stream` |
| Layer / kind | `algorithm` / `transform` |
| Capability | `language.generation.streaming` |

## Ports

| Port | Direction | Schema |
| --- | --- | --- |
| `text_in` | Input Text | Prompt or turn context, streaming |
| `text_out` | Output Text | Assistant response deltas, streaming |

## Configuration

`model` defaults to `qwen-flash`; `system_prompt` defines behavior; `temperature` defaults to
`0.6`. Sentence-sized output allows TTS to start before the full response is complete.
