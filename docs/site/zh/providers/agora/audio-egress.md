# Agora Audio Egress

把生成的 PCM Audio Frame 发布给 Agora 频道中的远端订阅者。

| 属性 | 值 |
| --- | --- |
| Node Type | `agora.audio_sink` |
| 层级 / 角色 | `transport` / `sink` |
| Capability | `rtc.audio.egress` |
| 语言 | C++ |

## Port

| Port | 方向 | Schema |
| --- | --- | --- |
| `audio_in` | 输入 Audio | PCM S16LE、48 kHz、单声道、20 ms、流式 |
| `signal_in` | 输入 Signal | 显式 Graph 控制边 |

配置共享的 `agora` Connection。收到 `muxiva.voice.speech.started` 后，这个 Node 会清空
待播放 PCM 队列，并推进取消序列水位。已经排在其他 Graph 队列中、随后迟到且序列不高于
该水位的音频也会被丢弃。该行为属于播放 Node；Core 不执行语音轮次过滤。

```text
tts.audio_out -> agora-egress.audio_in
```

如果 TTS 输出其他采样率，应在这个 Node 前连接 `builtin.audio_resampler`。
