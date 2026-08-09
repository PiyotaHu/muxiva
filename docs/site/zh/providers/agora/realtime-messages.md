# Agora 实时消息

`agora.data_source` 与 `agora.data_sink` 在音频所使用的同一个共享 RTC Session 中传输
客户端命令和 Agent 事件。它们是 C++ Node，不是新的 Core 抽象。

| Node | Port | Schema |
| --- | --- | --- |
| `agora.data_source` | `message_out` Byte 输出 | `application/vnd.muxiva.client-command+json`，最大 1 KiB |
| `agora.data_sink` | `message_in` Byte 输入 | 应用消息最大 32 KiB；传输单包最大 1 KiB |

数据流采用可靠、有序模式。Muxiva 会把出站数据包控制在 Agora 6 KiB/s 限制以内。
Voice Agent 示例中的项目 Node `voice_room.event_encoder` 把 `EventFrame` 映射为应用自己的
`muxiva.client-event/v1`；`agora.data_sink` 只负责传输层工作，超过单包大小时使用
`muxiva.transport-fragment/v1`，由客户端重组。

这个边界是刻意设计的：客户端 Schema 与语音取消策略属于应用 Node，RTC 单包限制属于
Agora 传输 Node，两者都不是 Runtime Core builtin。

所有 Agora Node 获取同一个进程级共享 Session，并使用配置的 Bot UID。第一版隔离模型
只接受配置好的 Browser UID，其他参与者的音频和消息会被忽略。

这条链路不经过 NotificationBus；NotificationBus 只保留为服务端本地可观测设施。
