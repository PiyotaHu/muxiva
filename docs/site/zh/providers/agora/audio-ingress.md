# Agora Audio Ingress

从 Agora 频道接收远端音频，并输出实时 PCM Frame。

| 属性 | 值 |
| --- | --- |
| Node Type | `provider.agora.audio_source` |
| 层级 / 角色 | `transport` / `transform` |
| Capability | `rtc.audio.ingress` |
| 语言 | C++ |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `tick_in` | 输入 Event | 用于排空有界 Native 接收队列的轮询 Tick |
| `audio_out` | 输出 Audio | PCM S16LE、16 kHz、单声道、20 ms、流式 |

Node 使用 Tick 驱动，因此 Native SDK 回调线程不会直接执行 Graph 逻辑。配置共享的
`agora` Connection 即可；v1 没有实例级配置。

```text
interval-tick.tick_out -> agora-ingress.tick_in
agora-ingress.audio_out -> asr.audio_in
```

停止或中止时会先关闭数据准入再离开频道，迟到的 Native 回调会被丢弃。
