# 阿里云 Qwen 算法层

Qwen 官方 Node 使用 Python 实现，直接调用 DashScope 官方 WebSocket 与 HTTP 协议，
不需要另外下载所谓的 Qwen SDK。

- 分类：`algorithm`
- 实现目录：`providers/algorithm/qwen/python`
- 区域：阿里云百炼中国（北京）
- 凭据申请：[百炼 API Key 与 Workspace ID 逐字段指南](../../voice-credentials.md#b-api-key-workspace-id)
- 安装教程：[真实语音 Agent 指南](../../voice-demo.md)

执行安装后，在 Studio **Connections** 中填写 API Key 和 Workspace ID：

```bash
./examples/voice-agent/setup.sh
muxiva doctor --voice
```

两者必须来自华北 2（北京）的同一业务空间。这些 Qwen Node 不需要下载厂商 SDK。

Node：

- [Qwen Audio Realtime](audio-realtime.md)
- [Qwen Server VAD + Streaming ASR](asr-realtime.md)
- [Qwen 可取消 Streaming LLM](llm-stream.md)
- [Qwen 可取消 Streaming TTS](tts-realtime.md)
