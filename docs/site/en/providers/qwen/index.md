# Alibaba Cloud Qwen algorithms

The official Qwen Nodes are implemented in Python against documented DashScope WebSocket and HTTP
protocols. There is no separate Qwen SDK download.

- Category: `algorithm`
- Implementation: `providers/algorithm/qwen/python`
- Region: Alibaba Cloud Model Studio China (Beijing)
- Credentials: [field-by-field API key and Workspace ID guide](../../voice-credentials.md#b-create-the-model-studio-key-and-workspace-id)
- Setup: [real voice Agent guide](../../voice-demo.md)

Run setup, then enter the API Key and Workspace ID in Studio **Connections**:

```bash
./examples/voice-agent/setup.sh
muxiva doctor --voice
```

Both values must belong to the same China (Beijing) workspace. These Qwen Nodes require no
vendor SDK download.

Nodes:

- [Qwen Audio Realtime](audio-realtime.md)
- [Qwen Streaming ASR](asr-realtime.md)
- [Qwen Streaming LLM](llm-stream.md)
- [Qwen Streaming TTS](tts-realtime.md)
