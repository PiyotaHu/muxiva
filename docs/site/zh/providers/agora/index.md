# Agora RTC 传输层

Agora 官方传输 Node 使用 C++ 实现。输入和输出 Node 共享同一个进程级 RTC Engine，将凭据隔离
在 Graph JSON 之外，并与 Voxa 交换类型化 PCM Audio Frame。

- 分类：`transport`
- SDK：Agora RTC Native SDK `4.6.2`
- 实现目录：`providers/transport/agora/cpp`
- 凭据申请：[Agora App ID 与两个 Token 逐字段指南](../../voice-credentials.md#a-agora-app-id-token)
- 安装教程：[真实语音 Agent 指南](../../voice-demo.md)

macOS 运行：

```bash
./examples/voice-agent/setup.sh
voxa doctor --voice
```

在 Studio **Connections** 中配置 App ID、频道、Voxa Bot UID/Token 和浏览器
UID/Token。开发环境使用短期 Token，生产环境必须使用 Token Server。

首次运行不要只生成一个 Token：浏览器固定使用数字 UID `1001`，Voxa Bot 使用 `2001`，
需要为同一个 Channel 分别生成两个 Token。App Certificate 只用于生成 Token，不进入 Voxa。

Node：

- [Agora Audio Ingress](audio-ingress.md)
- [Agora Audio Egress](audio-egress.md)
