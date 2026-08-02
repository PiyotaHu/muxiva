# 阿里云 Qwen 算法层

Qwen 是 Python Algorithm Provider，直接使用 DashScope 官方 WebSocket 与 HTTP 协议，
不需要另外下载所谓的 Qwen SDK。

- 分类：`algorithm`
- 实现目录：`providers/algorithm/qwen/python`
- 区域：阿里云百炼中国（北京）
- 安装教程：[真实语音 Agent 指南](../../voice-demo.md)

执行安装后，在 Studio **Connections** 中填写 API Key 和 Workspace ID：

```bash
./examples/voice-agent/setup.sh
voxa doctor --voice
```

Node：

- [Qwen Audio Realtime](audio-realtime.md)
- [Qwen Streaming ASR](asr-realtime.md)
- [Qwen Streaming LLM](llm-stream.md)
- [Qwen Streaming TTS](tts-realtime.md)
