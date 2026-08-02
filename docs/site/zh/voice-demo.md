# 旗舰语音 Demo

这是 Voxa 的真实语音体验入口，不是 Mock。浏览器采集麦克风并播放回复，Agora
Native C++ Node Pack 负责双向 RTC 音频，Qwen Python Node Pack 负责智能层，Rust
Runtime 只负责厂商无关的 Frame、Graph、Signal、EventBus、Turn 和调度。

## 选择一张图

| 图 | 适合什么体验 | 链路 |
| --- | --- | --- |
| **Qwen Realtime（推荐）** | 第一次运行、低延迟自然对话 | Audio → Qwen Audio Realtime → Audio |
| **Qwen Cascade** | 检查和替换每一个智能阶段 | VAD → ASR → LLM → TTS |

两张图都持续运行到你主动结束会话。Cascade 图在检测到用户开口时发送
`voxa.runtime.interrupt`；Runtime 推进全局 Turn，并在扬声器 Sink 前丢弃上一轮仍在
飞行的音频。

## 1. 准备环境

```bash
git clone https://github.com/PiyotaHu/Voxa.git
cd Voxa
cargo install --locked --path crates/voxa-cli
```

还需要：

- 当前平台的 Agora Native C++ SDK；
- 阿里云百炼 DashScope API Key 与 Workspace ID；
- 一个 Agora App ID 和 Channel；
- 同一 Channel 下三个不同 UID 的短期 Token。

| 身份 | 用途 | Token 能力 |
| --- | --- | --- |
| Browser | 浏览器麦克风与扬声器 | 发布和订阅 |
| Ingress Bot | C++ 接收浏览器音频 | 订阅 |
| Egress Bot | C++ 发布助手音频 | 发布 |

不要把 Agora App Certificate 写入仓库、Studio 或浏览器。

## 2. 安装应用 Node Pack

```bash
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
```

该命令安装 Qwen Python Node 依赖，并将两个 Agora C++ Node Pack 编译到项目的
`.voxa/native/` 目录。成功时最后会显示 `[VOXA][READY]`。

## 3. 启动 Studio

```bash
./examples/voice-agent/run.sh
```

Studio 会在本机打开。按顺序操作：

1. **Templates** → 选择 **Qwen Realtime**；
2. **Connections** → 填写 DashScope 和 Agora 字段；
3. **Voice Room** → 打开语音体验页面；
4. **Start live conversation** → 允许麦克风权限；
5. 自然讲话，并在助手说话时插话测试打断；
6. 完成后点击 **End session**。

Voice Room 会实时展示麦克风波形、用户字幕、助手回复、Graph 阶段、Node 调用和
Frame 活动。Realtime 跑通后，可返回 Studio 切换到 Cascade，对比两条链路。

## 凭据边界

DashScope Key、Workspace ID 和两个 Bot Token 只保留在本地 Studio 进程中。
浏览器只会取得 Manifest 明确允许的 App ID、Channel、Browser UID 和短期 Browser
Token。所有凭据都不应提交到 Git。

## 验证开发环境

不使用真实凭据也可以验证代码、ABI 和图模板是否完整：

```bash
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

这只是工程门禁，不会伪装成真实通话。完整验收必须实际加入 Agora Channel、对着
麦克风讲话、听到 Qwen 回复，并在播放期间成功插话。
