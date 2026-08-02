# Provider and Node catalog

Voxa separates two independent properties of every Node:

- **Layer** describes architecture: `transport`, `algorithm`, `media`, `control`, or `utility`.
- **Kind** describes graph behavior: `source`, `transform`, or `sink`.

Provider metadata, credentials, SDK compatibility, license, and documentation live once in
`voxa.provider.json`. Each `voxa.node.json` declares a stable capability and exact Port schemas.

| Layer | Provider | Capabilities |
| --- | --- | --- |
| Transport | [Agora RTC](agora/index.md) | RTC audio ingress and egress |
| Algorithm | [Alibaba Cloud Qwen](qwen/index.md) | Realtime speech, ASR, LLM, and TTS |
| Media, control, utility | [Voxa built-ins](builtin.md) | Resampling, VAD, turn context, clocks, and diagnostics |

Studio discovers configured Provider Roots recursively. Use the Palette filters to browse by
layer or search by provider, capability, tag, or Node type.
