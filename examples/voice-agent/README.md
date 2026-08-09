# Muxiva Flagship Voice Agent

[中文](#中文) · [English](#english)

## 中文

这是 Muxiva 的真实门面应用，不是 Mock：浏览器通过 Agora Web SDK 采集和播放语音，
Agora Native C++ Node Pack 负责 RTC，Qwen Python Node 负责实时模型、ASR 与 TTS，
独立发布的 Pi TypeScript 编码 Agent 负责 Demo 2 的会话、Tool 与受限文件操作，Rust Runtime 只处理厂商无关的
Frame、Graph、Signal、NotificationBus、Turn 与调度。

Studio 内置两张可选择的图：

- **Qwen Realtime（推荐）**：Qwen Audio Realtime 端到端语音模型，链路短、延迟低；
- **Pi Agent Full-Duplex Cascade（Demo 2）**：阿里云 Qwen Server VAD + Streaming ASR →
  Pi TypeScript 编码 Agent（Qwen 模型、会话、Tool Call 与工作区文件）→ Speech Formatter →
  可取消 Qwen TTS。聊天框保留原始 Markdown，只有清理后的自然文本进入 TTS。用户插话时，
  `muxiva.voice.speech.started` Signal 会取消旧 Agent/TTS 请求、清除过期文本和客户端
  事件，并清空 Agora 播放队列。

### 准备

1. 安装 `muxiva` 发布版二进制，或在源码仓库执行
   `cargo install --path crates/muxiva-cli --locked`。
2. 安装 Node.js 22.19 或更高版本；它用于运行 Demo 2 的 TypeScript Agent Node。
3. 从 Agora 官方渠道取得目标平台 Native C++ SDK。
4. 在阿里云百炼取得 API Key 与 Workspace ID；在 Agora 为同一 Channel 生成浏览器
   与 Bot 两个短期 RTC Token。不要把 App Certificate 放进 Studio 或浏览器。
5. 构建应用 Node Pack，并拉取锁定的
   [PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent) 版本：

```sh
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
```

### 体验

```sh
./examples/voice-agent/run.sh
```

在 Studio 中按顺序操作：

1. **Templates** → 选择 Realtime 或 Cascade；
2. **Connections** → 填写 DashScope 和 Agora 配置；
3. **Voice Room**（会先自动保存当前有效图）；
4. 点击 **Start live conversation**，允许麦克风权限，然后自然说话、在助手说话时插话。

Cascade 默认使用 `vad_threshold: 0.45`。在 Studio 画布选择 `qwen-vad-asr`，即可在
**Configuration** 中修改灵敏度；修改后点击 **Validate** 和 **Save graph**。

Voice Room 会持续运行，直到点击 **End session**；页面实时展示 Graph、Node 调用、
Frame 数和各阶段活动状态。Secret 只保留在本地 Studio 进程内存；只有 Manifest
显式标记为浏览器必需的 App ID、Channel、Web UID 和短期 Web Token 才能通过本地
鉴权接口交给 Voice Room。

离线验收：

```sh
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

后者会运行 Qwen 协议测试、构建 C++ Node Pack、通过真实动态 ABI 加载二进制，并用
Studio 的真实 Registry 编译两张模板。带凭据的实房仍属于发布前人工认证门禁。

## English

This is Muxiva's credentialed flagship application, not a mock. The browser uses
Agora Web SDK for capture and playback, native C++ Node Packs own RTC, Python
Nodes own Qwen speech APIs, and an independently versioned TypeScript Pi coding
Agent owns Demo 2 sessions, tools, and workspace-scoped file edits. The Rust
Runtime remains vendor-neutral.

Studio offers two graphs: low-latency **Qwen Realtime**, and **Pi Agent
Full-Duplex Cascade (Demo 2)**: Qwen Server VAD + Streaming ASR → a stateful,
tool-using Pi TypeScript coding Agent backed by Qwen → Speech Formatter → cancellable Qwen TTS.
The chat retains original Markdown while only normalized spoken text reaches TTS. On
barge-in, the `muxiva.voice.speech.started` Signal cancels Agent/TTS work,
stale text and client events, and Agora playback.

Prepare Node.js 22.19+, an Agora Native C++ SDK, DashScope API Key and Workspace
ID, plus short-lived RTC tokens for browser and bot identities.
Never put an Agora App Certificate in Studio or a browser. Then run:

```sh
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
./examples/voice-agent/run.sh
```

In Studio choose **Templates**, fill **Connections**, open **Voice Room**, and
select **Start live conversation**. The room remains live until you end it and
shows graph, callback, frame, and pipeline activity in real time.

Cascade defaults to `vad_threshold: 0.45`. Select `qwen-vad-asr` on the Studio canvas to edit
the value in **Configuration**, then select **Validate** and **Save graph**.

Run `./scripts/check-provider-boundaries.sh` and
`./scripts/check-voice-node-packs.sh` for the offline architecture, protocol,
native loading, and template compilation gates. A credentialed live-room run is
still a release certification step because credentials and the vendor SDK are
not stored in this repository.
