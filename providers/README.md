# Muxiva Providers

Provider integrations live outside the vendor-neutral Rust runtime and are
grouped by vendor and implementation language:

- `algorithm/qwen/python`: Realtime, ASR, streaming LLM, and TTS Python Node Packs.
- `transport/agora/cpp`: RTC transport, C++ Node Packs, native adapter, and tests.

Each provider owns one `muxiva.provider.json`. It declares vendor metadata, SDK
compatibility, documentation, licensing, and shared Connections once. Each
Node then declares an orthogonal architecture `category`, graph `kind`, stable
`capability`, and typed Port schemas in its `muxiva.node.json`.

Setup guides:

- [Qwen Provider](../docs/providers/qwen.md)
- [Agora Provider](../docs/providers/agora.md)

Applications reference one or more Node Pack roots through
`.muxiva/providers.json`. Provider source remains inspectable but read-only in
Studio; application-owned Nodes continue to live in `.muxiva/nodes` and can be
edited in Node Lab.
