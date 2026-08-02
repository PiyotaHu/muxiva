# 端到端语音链路

一条真实语音链路最能说明 Voxa 的分层价值：浏览器处理用户体验，Agora 处理网络传输，
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
    QR -."字幕 Event".-> UI["Studio / Voice Room"]
```

适合优先追求低延迟、自然轮次和较少组件的应用。

### 级联图

级联图把能力拆开，便于分别选择模型、观察中间结果和插入业务逻辑：

```mermaid
flowchart LR
    IN["Agora Ingress"] --> VAD["VAD"]
    IN --> ASR["Qwen ASR"]
    VAD --> FUSION["Turn / Context"]
    ASR --> FUSION
    FUSION --> LLM["Qwen LLM"]
    LLM --> TEXT["字幕 / Tool"]
    LLM --> TTS["Qwen TTS"]
    TTS --> OUT["Agora Egress"]
```

适合需要自定义 VAD、提示词、工具、审核、字幕或 TTS 的应用。分支与汇合是 Graph 的
正常能力，不是只能运行 `A → B → C` 的线性 Pipeline。

## 每一层到底做什么

| 层 | 实现 | 职责 | 不负责 |
| --- | --- | --- | --- |
| 项目 Web | HTML/JS + Agora Web SDK | 麦克风权限、频道、播放、交互 UI | 模型密钥与 Runtime 调度 |
| Transport Provider | Agora C++ Node Pack | RTC 收发与 PCM Frame 转换 | ASR、LLM、Graph 调度 |
| Runtime Core | Rust | 类型、队列、并发、Turn、打断、关闭 | 厂商请求与产品 UI |
| Algorithm Provider | Qwen Python Node Pack | Realtime 或 ASR/LLM/TTS 流 | RTC 频道与 Edge Queue |
| 开发工具 | CLI + Studio | 创建、配置、校验、运行、观测 | 生产用户界面 |

## 全双工与 Barge-in

全双工不是“同时开两个 Socket”就完成。用户插话时需要跨层协作：

```mermaid
sequenceDiagram
    participant U as 用户
    participant T as Agora Transport
    participant R as Rust Runtime
    participant M as Qwen Provider
    participant P as 播放端

    M->>R: 当前 Turn 的音频 Frame
    R->>T: 发送播放音频
    T->>P: 播放 Agent 回答
    U->>T: 用户在播放期间开口
    T->>R: 新音频 Frame
    R->>R: VAD/模型确认 Barge-in，切换 Turn
    R-->>M: Interrupt / Cancel Signal
    R-->>T: 停止旧 Turn 音频
    R->>M: 开始新 Turn
```

Provider 尽力取消远端生成，Runtime 同时用 Turn 身份过滤晚到结果；即使供应商的取消
存在网络延迟，旧音频也不能重新进入当前播放链路。

## 凭据与部署边界

- Agora 临时 RTC Token 可以按最小权限提供给浏览器；生产环境由 Token Server 签发；
- Qwen API Key 和 Workspace ID 只存在于服务端 Connections/环境变量；
- Graph 与 Node Manifest 可以提交 Git，但不包含真实密钥；
- Studio 只用于本地开发，生产网页通过明确的应用服务边界连接 Runtime。

要亲自跑通两张图，请按照[从零运行真实语音 Agent](voice-demo.md)逐步操作。
