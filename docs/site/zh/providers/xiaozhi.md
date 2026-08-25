# 小智 ESP32 语音交互

Muxiva 原生支持开源的 [小智 ESP32](https://github.com/78/xiaozhi-esp32)
语音助手设备作为客户端。小智开发板通过其原生 WebSocket + Opus 协议连接到
Muxiva 语音图，即可获得完整的 **VAD + ASR + LLM + TTS** 语音管线，无需改动固件。

- 传输 Provider：`providers/transport/xiaozhi`（Python）
- 分类：`transport`
- 设备协议：小智 WebSocket `v1`（JSON 控制 + Opus 音频）
- 旗舰示例：[`examples/xiaozhi-agent`](https://github.com/PiyotaHu/muxiva/tree/main/examples/xiaozhi-agent)
- 凭据：阿里云百炼 API Key + Workspace ID
- 计费：请查看百炼最新价格和额度文档；本文不固化会变化的价格信息

## 设备协议

小智固件通过一条 WebSocket 连接收发 JSON 控制消息与 Opus 音频：

| 方向 | 消息 | 含义 |
| --- | --- | --- |
| 设备 → 服务端 | `{"type":"hello"}` | 握手；服务端回复协商后的 Opus 音频参数 |
| 设备 → 服务端 | 二进制 Opus 包 | 麦克风音频（60ms 帧） |
| 设备 → 服务端 | `{"type":"abort"}` | 用户按下打断按钮 |
| 设备 → 服务端 | `{"type":"listen",...}` | 设备拾音状态切换 |
| 设备 → 服务端 | `{"type":"ping"}` | 保活；服务端回复 `pong` |
| 服务端 → 设备 | `{"type":"hello",...}` | 协商的 `audio_params` 与 `session_id` |
| 服务端 → 设备 | 二进制 Opus 包 | 助手语音 |
| 服务端 → 设备 | `{"type":"stt","text":...}` | 用户转写在设备屏幕展示 |
| 服务端 → 设备 | `{"type":"tts","state":...}` | 助手说话状态 / 回答文字 |

因此设备屏幕会实时展示：ASR 识别的问题（`stt`）、LLM 回答（`tts sentence_start`），
以及说话 / 打断状态（`tts start` / `stop`）。

## 架构

传输层由三个 Node Pack 组成，与 Agora RTC Provider 处于同一架构层：

- **`xiaozhi.audio_source`**（源节点）：内嵌 WebSocket 服务端，将 Opus 解码为
  16kHz PCM，转发设备控制，并对下行音频做缓冲和实时节拍发送。
- **`xiaozhi.audio_sink`**（汇节点）：把 TTS PCM 编码回 Opus 流式下发到设备。
- **`xiaozhi.event_encoder`**（汇节点）：把转写、经语音呈现处理的助手文字、TTS
  生命周期、设备命令和产品情绪映射为设备协议消息。

情绪不是 Agent 输出。`emotion_rules` 与 `default_emotion` 由小智 Graph 配置；规则为空时
encoder 不发送情绪消息。这样角色词表和显示策略可以替换，而无需修改 Agent 或 Provider 代码。

由于 Muxiva 每个 Python Node 运行在独立进程中，源节点内置一个小型 gateway，
汇节点与事件编码节点通过回环 JSON-lines 控制 socket 连接它。跨运行时边界流动的
只有 PCM Frame 与控制 Signal/Event；Opus 与 WebSocket 协议始终留在传输 Provider 内部。

## 示例图

```text
ESP32（Opus over WebSocket）
        │  ws://<服务器IP>:8888
        ▼
xiaozhi.audio_source ──► qwen.asr_realtime ──► builtin.voice_turn_controller ──► pi.agent
   (Opus 网关)             (VAD + ASR 事实)          (准入 + 唯一取消)             (工具 + 模型)
        ▲                                                                              │
        │                                                                              ▼
xiaozhi.audio_sink ◄── builtin.audio_resampler ◄── qwen.tts_realtime
        ▲                        │                     ▲
        └────────────────────────┴── builtin.speech_formatter
```

该图支持全双工对话。原始 VAD 不删除播放队列；只有通过最终转写准入或设备强制停止后，
Voice Turn Controller 才会用一个标准 Signal 取消正在进行的 TTS/Agent/播放工作。

## 快速开始（树莓派 4B）

```bash
cd examples/xiaozhi-agent
./setup.sh                     # 安装 libopus、websockets、Qwen 依赖并生成 .env
./run.sh                       # 启动 muxiva serve；WebSocket 监听 0.0.0.0:8888
```

把固件 WebSocket 地址指向 `ws://<树莓派IP>:8888` 即可对话。

## 自动化全双工测试

`examples/xiaozhi-agent/tests/test_full_duplex.py` 无需任何硬件即可复现三轮对话
（打招呼、讲笑话、天气打断）。它用 Qwen TTS 合成用户语音，以 Opus 流式送入服务端
（与设备麦克风完全一致），并校验 `stt` / `tts` 展示序列与打断信号。完整命令见
[示例 README](https://github.com/PiyotaHu/muxiva/tree/main/examples/xiaozhi-agent)。
