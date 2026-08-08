# 端到端语音链路

一条真实语音链路最能说明 Muxiva 的分层价值：浏览器处理用户体验，Agora 处理网络传输，
Qwen 处理算法，Rust Core 处理实时调度。任何一层都不需要知道其他层的内部实现。

## 两张可选 Graph

### Realtime 模型图

Realtime 模型把语音理解与生成放在一个流式模型会话中，链路短、交互自然：

```mermaid
flowchart LR
    B["浏览器麦克风"] --> AI["Agora Audio Ingress<br/>C++ · Transport"]
    AI --> QR["Qwen Audio Realtime<br/>Python · Algorithm"]
    QR --> AO["Agora Audio Egress<br/>C++ · Transport"]
    AO --> S["浏览器扬声器"]
    QR --> CE["客户端事件编码<br/>Rust · Protocol"]
    CE --> DO["Agora Data Egress<br/>C++ · Transport"]
    DO --> UI["独立 Voice Client"]
```

适合优先追求低延迟、自然轮次和较少组件的应用。

### 级联图

级联图把能力拆开，便于分别选择模型、观察中间结果和插入业务逻辑：

```mermaid
flowchart LR
    IN["Agora Ingress"] --> ASR["Qwen Server VAD + Streaming ASR"]
    ASR --> FUSION["Turn Context / Policy"]
    FUSION --> LLM["可取消 Qwen LLM Worker"]
    CLOCK["20 ms Async Tick"] --> LLM
    LLM --> GATE["文本取消水位 / Tool"]
    GATE --> TTS["可取消 Qwen TTS Worker"]
    CLOCK --> TTS
    TTS --> OUT["Agora Egress"]
    ASR -. "speech.started Signal" .-> LLM
    ASR -. "speech.started Signal" .-> TTS
    ASR -. "speech.started Signal" .-> GATE
    ASR -. "speech.started Signal" .-> OUT
```

Demo 2 默认全部使用阿里云模型：Qwen ASR 会同时执行 Server VAD 和流式转写；Qwen LLM
与 Qwen TTS 的厂商 I/O 在各自后台 Worker 中运行，通用 `interval_tick` 只负责让 Node
以短回调排空有界结果队列，使 `on_signal` 不会被长网络请求堵住。各阶段仍可替换，分支
与汇合是 Graph 的正常能力，不是只能运行 `A → B → C` 的线性 Pipeline。

## 每一层到底做什么

| 层 | 实现 | 职责 | 不负责 |
| --- | --- | --- | --- |
| 项目 Web | HTML/JS + Agora Web SDK | 麦克风权限、频道、播放、交互 UI | 模型密钥与 Runtime 调度 |
| Agora 官方 Node | C++ Node Pack | 单一共享 RTC Session、音频收发、可靠有序客户端消息 | ASR、LLM、Graph 调度 |
| Runtime Core | Rust | 类型、队列、并发、透明 Signal 路由、关闭 | 厂商请求、语音 Turn 或产品 UI |
| Qwen 官方 Node | Python Node Pack | Realtime 或 ASR/LLM/TTS 流 | RTC 频道与 Edge Queue |
| 开发工具 | CLI + Studio | 创建、配置、校验、运行、观测 | 生产用户界面 |

## 全双工与 Barge-in

全双工不是“同时开两个 Socket”就完成。用户插话时需要跨层协作：

```mermaid
sequenceDiagram
    participant U as 用户
    participant T as Agora Nodes
    participant R as Rust Runtime
    participant M as Qwen Node
    participant P as 播放端

    M->>R: 回答音频 Frame
    R->>T: 发送播放音频
    T->>P: 播放 Agent 回答
    U->>T: 用户在播放期间开口
    T->>R: 新音频 Frame
    M->>M: 模型确认用户正在说话并取消当前回答
    M-->>R: muxiva.voice.speech.started Signal
    R-->>T: 透明转发 Signal
    T->>T: Audio Sink 清空尚未播放的音频
    R->>M: 后续音频继续进入同一 Node
```

打断语义完全属于 Node。Realtime 图由 Qwen Audio Node 取消远端回答；级联图由 Qwen
ASR Server VAD 发出同名 Signal，Qwen LLM 关闭 HTTP SSE、Qwen TTS 关闭当前 WebSocket
并清空待合成文本与 PCM，文本门和客户端事件编码器推进取消水位，Agora Audio Sink
清空播放队列并拒绝迟到音频。Core 不理解语音、Turn 或具体 Signal 名称，只负责路由
不透明 Signal。

## 客户端数据不是 Studio 遥测

ASR、Agent 文字和说话状态都从 Graph 进入 `agora.data_sink`。浏览器从 Agora 可靠有序
数据流接收 `muxiva.client-event/v1`，不再轮询 `/api/v1/runtime/events`，也不能启停 Runtime。
EventBus 继续作为进程内日志、指标和 Studio 运维观测设施，但不是终端用户协议。

本地 `/api/v1/client/session` 只负责给浏览器提供临时 RTC 启动配置。生产部署应替换为
自己的鉴权与短期 Token 服务，媒体和消息链路无需改变。

第一版会话隔离采用严格模型：一个 Agora Channel 对应一个 Agent Session 和一个配置好的
浏览器 UID。共享 C++ Session 会丢弃其他 UID 的媒体与消息，避免错误混合多名参与者。

!!! note "级联取消边界"
    Demo 2 已能主动关闭进行中的 LLM HTTP SSE 与 TTS WebSocket，并通过三层水位过滤
    晚到结果。已经发进 Agora 网络或浏览器播放缓冲区的 PCM 无法撤回，因此 Audio Sink
    仍使用短包和有界队列；“硬打断”指取消服务端连接与本地流水线，不代表逆转已发送媒体。

## 凭据与部署边界

- Agora 临时 RTC Token 可以按最小权限提供给浏览器；生产环境由 Token Server 签发；
- Qwen API Key 和 Workspace ID 只存在于服务端 Connections/环境变量；
- Graph 与 Node Manifest 可以提交 Git，但不包含真实密钥；
- Studio 只用于本地开发，生产网页通过明确的应用服务边界连接 Runtime。

要亲自跑通两张图，请按照[从零运行真实语音 Agent](voice-demo.md)逐步操作。
