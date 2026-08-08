# Muxiva Flagship Voice Agent

[中文](#中文) · [English](#english)

## 中文

这是 Muxiva 的真实门面应用，不是 Mock：浏览器通过 Agora Web SDK 采集和播放语音，
Agora Native C++ Node Pack 负责 RTC，Qwen Python Node Pack 负责智能层，Rust Runtime
只处理厂商无关的 Frame、Graph、Signal、EventBus、Turn 与调度。

Studio 内置两张可选择的图：

- **Qwen Realtime（推荐）**：Qwen Audio Realtime 端到端语音模型，链路短、延迟低；
- **Qwen Full-Duplex Cascade（Demo 2）**：阿里云 Qwen Server VAD + Streaming ASR →
  可取消 Qwen LLM → 可取消 Qwen TTS。用户插话时，`muxiva.voice.speech.started`
  Signal 会取消旧 LLM/TTS 请求、清除过期文本和客户端事件，并清空 Agora 播放队列。

### 准备

1. 安装 `muxiva` 发布版二进制，或在源码仓库执行
   `cargo install --path crates/muxiva-cli --locked`。
2. 从 Agora 官方渠道取得目标平台 Native C++ SDK。
3. 在阿里云百炼取得 API Key 与 Workspace ID；在 Agora 为同一 Channel 生成三个
   短期 RTC Token：浏览器 UID、Ingress Bot UID、Egress Bot UID。不要把 App
   Certificate 放进 Studio 或浏览器。
4. 构建并安装应用 Node Pack：

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
Node Packs own Qwen intelligence, and the Rust Runtime remains vendor-neutral.

Studio offers two graphs: low-latency **Qwen Realtime**, and **Qwen Full-Duplex
Cascade (Demo 2)**: Alibaba Cloud Qwen Server VAD + Streaming ASR → cancellable
Qwen LLM → cancellable Qwen TTS. On barge-in, the
`muxiva.voice.speech.started` Signal cancels the old LLM/TTS work, stale text
and client events, and Agora playback.

Prepare an Agora Native C++ SDK, DashScope API Key and Workspace ID, plus three
short-lived RTC tokens for browser, ingress bot, and egress bot identities.
Never put an Agora App Certificate in Studio or a browser. Then run:

```sh
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
./examples/voice-agent/run.sh
```

In Studio choose **Templates**, fill **Connections**, open **Voice Room**, and
select **Start live conversation**. The room remains live until you end it and
shows graph, callback, frame, and pipeline activity in real time.

Run `./scripts/check-provider-boundaries.sh` and
`./scripts/check-voice-node-packs.sh` for the offline architecture, protocol,
native loading, and template compilation gates. A credentialed live-room run is
still a release certification step because credentials and the vendor SDK are
not stored in this repository.
