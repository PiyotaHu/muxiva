# Agora RTC 传输层

Agora 是 C++ Transport Provider。它使用独立的输入端和输出端客户端加入频道，将凭据隔离
在 Graph JSON 之外，并与 Voxa 交换类型化 PCM Audio Frame。

- 分类：`transport`
- SDK：Agora RTC Native SDK `4.6.2`
- 实现目录：`providers/transport/agora/cpp`
- 安装教程：[真实语音 Agent 指南](../../voice-demo.md)

macOS 运行：

```bash
./examples/voice-agent/setup.sh
voxa doctor --voice
```

在 Studio **Connections** 中配置 App ID、频道、输入 UID/Token、输出 UID/Token 和浏览器
UID/Token。开发环境使用短期 Token，生产环境必须使用 Token Server。

Node：

- [Agora Audio Ingress](audio-ingress.md)
- [Agora Audio Egress](audio-egress.md)
