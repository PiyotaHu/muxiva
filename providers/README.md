# Voxa Providers

Provider integrations live outside the vendor-neutral Rust runtime and are
grouped by vendor and implementation language:

- `qwen/python`: Realtime, ASR, streaming LLM, and TTS Python Node Packs.
- `agora/cpp`: RTC transport, C++ Node Packs, native adapter, and tests.

Applications reference one or more Node Pack roots through
`.voxa/providers.json`. Provider source remains inspectable but read-only in
Studio; application-owned Nodes continue to live in `.voxa/nodes` and can be
edited in Node Lab.
