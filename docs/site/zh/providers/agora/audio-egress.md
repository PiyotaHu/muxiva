# Agora Audio Egress

把生成的 PCM Audio Frame 发布给 Agora 频道中的远端订阅者。

| 属性 | 值 |
| --- | --- |
| Node Type | `provider.agora.audio_sink` |
| 层级 / 角色 | `transport` / `sink` |
| Capability | `rtc.audio.egress` |
| 语言 | C++ |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_in` | 输入 Audio | PCM S16LE、16 kHz、单声道、20 ms、流式 |

配置共享的 `agora` Connection。Voxa 会在 Sink 前执行旧轮次过滤，所以打断发生后，旧轮次
音频不会继续发布。

```text
tts.audio_out -> agora-egress.audio_in
```

如果 TTS 输出其他采样率，应在这个 Node 前连接 `builtin.audio_resample`。
