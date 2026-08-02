# Alibaba Cloud Qwen algorithms

Qwen is a Python algorithm Provider implemented against documented DashScope WebSocket and HTTP
protocols. There is no separate Qwen SDK download.

- Category: `algorithm`
- Implementation: `providers/algorithm/qwen/python`
- Region: Alibaba Cloud Model Studio China (Beijing)
- Setup: [real voice Agent guide](../../voice-demo.md)

Run setup, then enter the API Key and Workspace ID in Studio **Connections**:

```bash
./examples/voice-agent/setup.sh
voxa doctor --voice
```

Nodes:

- [Qwen Audio Realtime](audio-realtime.md)
- [Qwen Streaming ASR](asr-realtime.md)
- [Qwen Streaming LLM](llm-stream.md)
- [Qwen Streaming TTS](tts-realtime.md)
