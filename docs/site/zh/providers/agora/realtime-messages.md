# Agora 实时消息

`agora.data_source` 与 `agora.data_sink` 在音频所使用的同一个共享 RTC Session 中传输
客户端命令和 Agent 事件。它们是 C++ Node，不是新的 Core 抽象。

| Node | Port | Schema |
| --- | --- | --- |
| `agora.data_source` | `message_out` Byte 输出 | `application/vnd.voxa.client-command+json`，最大 1 KiB |
| `agora.data_sink` | `message_in` Byte 输入 | `application/vnd.voxa.client-event+json`，最大 1 KiB |

数据流采用可靠、有序模式。Voxa 会把出站消息控制在 Agora 6 KiB/s 限制以内。
`builtin.client_event_encoder` 把 `EventFrame` 编码成 `voxa.client-event/v1`；超过单包大小
时使用 `voxa.transport-fragment/v1`，由客户端重组。

所有 Agora Node 获取同一个进程级共享 Session，并使用配置的 Bot UID。第一版隔离模型
只接受配置好的 Browser UID，其他参与者的音频和消息会被忽略。

这条链路不经过 EventBus；EventBus 只保留为服务端本地可观测设施。
