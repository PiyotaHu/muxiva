# Node catalog

Voxa separates two independent properties of every Node:

- **Layer** describes architecture: `transport`, `algorithm`, `media`, `control`, or `utility`.
- **Kind** describes graph behavior: `source`, `transform`, or `sink`.

Voxa has one public extension unit: the **Node**. Each `voxa.node.json` declares a stable Node
type, capability, configuration, and exact Port schemas. A Connection only lets several Nodes
reuse the same local credentials.

| Layer | Official Node collection | Capabilities |
| --- | --- | --- |
| Transport | [Agora RTC](agora/index.md) | RTC audio ingress and egress |
| Algorithm | [Alibaba Cloud Qwen](qwen/index.md) | Realtime speech, ASR, LLM, and TTS |
| Media, control, utility | [Voxa built-ins](builtin.md) | Resampling, VAD, context, and diagnostics |

Studio discovers official Nodes and project Nodes under `.voxa/nodes/` recursively. Use the
Palette filters to browse by layer or search by capability, tag, or Node type.
