# Node catalog

Muxiva separates two independent properties of every Node:

- **Layer** describes architecture: `transport`, `algorithm`, `media`, `control`, or `utility`.
- **Kind** describes graph behavior: `source`, `transform`, or `sink`.

Muxiva has one public extension unit: the **Node**. Each `muxiva.node.json` declares a stable Node
type, capability, configuration, and exact Port schemas. A Connection only lets several Nodes
reuse the same local credentials.

| Layer | Official Node collection | Capabilities |
| --- | --- | --- |
| Transport | [Agora RTC](agora/index.md), [Xiaozhi ESP32](xiaozhi.md) | RTC audio plus ordered client-message ingress and egress; Xiaozhi device WebSocket + Opus voice interaction |
| Algorithm | [Alibaba Cloud Qwen](qwen/index.md) | Realtime speech, ASR, LLM, and TTS |
| Media, control, utility | [Muxiva built-ins](builtin.md) | Resampling, VAD, context, and diagnostics |

Studio discovers official Nodes and project Nodes under `.muxiva/nodes/` recursively. Use the
Palette filters to browse by layer or search by capability, tag, or Node type.
