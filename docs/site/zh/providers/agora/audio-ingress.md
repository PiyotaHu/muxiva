# Agora Audio Ingress

从 Agora 频道接收远端音频，并输出实时 PCM Frame。

| 属性 | 值 |
| --- | --- |
| Node Type | `agora.audio_source` |
| 层级 / 角色 | `transport` / `source` |
| Capability | `rtc.audio.ingress` |
| 语言 | C++ |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_out` | 输出 Audio | PCM S16LE、16 kHz、单声道、20 ms、流式 |

Native SDK 回调只把音频写入有界队列；Source 通过
`ctx.schedule_next_tick(20ms)` 在内部安排下一次排空，不在 SDK 回调线程执行 Graph
逻辑，也不需要暴露 `rtc-clock` 或 `tick_in`。配置共享的 `agora` Connection 即可；
v1 没有实例级配置。

```text
agora-ingress.audio_out -> asr.audio_in
```

停止或中止时会先关闭数据准入再离开频道，迟到的 Native 回调会被丢弃。

从旧版升级后需要重新执行 `./examples/voice-agent/setup.sh`：Source 的 Factory Version
已从 `1.0.0` 升到 `1.1.0`，用于明确区分旧的外部 Tick 契约与新的内部自调度契约。
