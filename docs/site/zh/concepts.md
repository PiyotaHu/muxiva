# Muxiva 系统全景与核心概念

Muxiva 是一套**实时多模态 Agent Runtime**。开发者用一张类型化 Graph 描述音频、
视频、文本、字节和控制消息如何流动；Rust Core 统一负责校验、调度、并发、背压、
生命周期、关闭和可观测性；ASR、LLM、TTS、RTC 等具体能力则由可替换 Node 提供。

理解 Muxiva 最重要的一句话是：**Core 定义稳定的运行机制，Node 实现可替换的业务能力，
Graph 把两者组合成可执行系统。**

## 系统全景

![Muxiva 系统全景](assets/architecture/muxiva-system-overview.png)

[下载可编辑的 Draw.io 源文件](assets/architecture/muxiva-system-overview.drawio)

图从上到下分为五层。阅读时先看层与层之间的边界，再看连线：蓝色实线表示数据或
调用关系，品红虚线表示 Signal 控制，灰色点线表示进程内 NotificationBus 可观测信息。

### 1. 产品与开发者入口

- **`muxiva` CLI** 创建、校验、运行和诊断项目，适合终端、脚本与 CI。
- **Muxiva Studio** 编辑 Graph、连接 Port、查看 Node 源码、配置本地 Connection，并观察
  当前 Runtime。
- **多语言 SDK** 让 Rust、C++、Python 和 TypeScript 开发者创建 Graph 或实现 Node。
- **项目 Web / Voice Room** 面向最终用户，负责麦克风、扬声器、聊天和 Barge-in 展示。

项目 Web 是独立客户端，不是 Studio 的一部分。它通过 RTC 或应用 Transport 与 Agent
通信，不调用 Runtime 生命周期接口，也不轮询进程内 NotificationBus。

### 2. 定义、发现与配置

这一层回答三个问题：**运行什么、去哪里找实现、怎样提供配置。**

- **Graph v1** 声明 Node、具名 Port、Edge、类型、队列容量和策略，是设计图，不包含
  线程、Socket、密钥或正在运行的对象。
- **Node Manifest** 描述 `node_type`、语言、Factory 版本、能力、配置 Schema 和精确
  输入输出 Schema。
- **Registry 与 Discovery** 汇集内置、官方和项目 Node，并选择 Graph 指定的精确
  Factory；Runtime 不根据相似名称猜实现。
- **Connections 与 Secrets** 为多个 Node 提供本地连接配置。开发期值可以保存在被
  Git 忽略的项目 `.env`，生产环境应由 Secret / Token 服务注入。

### 3. Vendor-neutral Rust Runtime Core

这是 Muxiva 的稳定内核，完全不依赖 Agora、Qwen 或其他厂商。

- **Graph Compiler** 在创建任何 Node 前检查 Schema、拓扑、Port 方向与兼容性，并把
  声明式 Graph 物化为可执行计划。
- **Concurrent Graph Runtime** 管理 `prepare → process → signal → finish / abort`、
  Worker 调度、取消和有界关闭。
- **数据面**使用不可变 Frame、类型化 Port 和有界 Edge Queue，处理音频、视频、文本
  与字节，并以背压约束延迟和内存。
- **控制与可观测面**路由相邻 Signal，并把 Event 发布给进程内观察者。Core 提供机制，
  但不硬编码“打断”“Turn”或某个模型厂商的业务策略。

### 4. 统一 Node 扩展层

Muxiva 的可执行扩展只有一个概念：**Node**。所谓“内置集成”或“厂商适配”都是遵守同一
契约的 Node，而不是另一种 Runtime 实体。

- **Rust 内置 Node** 适合重采样、VAD、取消门和通用工具等基础能力。
- **Python Node Host** 适合 Qwen Realtime、ASR、LLM、TTS 和快速迭代的算法逻辑。
- **C++ ABI Node Pack** 适合 Agora RTC、Codec、设备 SDK 等原生集成。
- **TypeScript / 项目 Node** 位于 Agent 项目的 `.muxiva/nodes/`，通过受管理的异步 Host
  运行，并使用同一套 Manifest、Factory 与 Frame 契约。

语言 Host 负责隔离线程、对象和异常；Graph 看见的始终是相同的 Node、Port 与 Frame，
不会因为实现语言改变运行语义。

#### Agent 是可复用 Node 能力，不是 Core 原语

Agent 在文本模型之上增加长生命周期会话、Tool Call、Steering 和厂商相关流式行为。
这些策略变化快，不应该写进 Rust Core。Muxiva 因此提供厂商无关的
`@muxiva/agent` TypeScript 契约，用稳定的 Prompt、Tick、取消、Text 和生命周期
Event Port 包装不同实现。Driver 可以接 Pi、其他 Agent Harness 或项目代码，而不改变
Graph。

Demo 2 验证了这条边界：Qwen ASR 输出完整问题，项目内的
[Pi Agent Node](nodes/pi-agent.md)管理会话与工具，原始 Markdown 分叉到客户端，
`builtin.speech_formatter` 再为 Qwen TTS 派生自然纯文本。Pi 只是可选 TypeScript
依赖；Runtime 看见的仍是一组普通 Node。

### 5. 外部服务与生产边界

外部模型、RTC 网络和 Token 服务不属于 Core。图中的语音应用使用 Alibaba Cloud Model
Studio 和 Agora 只是一个组合示例：开发者可以替换这些 Node，而无需修改 Runtime。
浏览器只拿短期 RTC Token，模型密钥只存在于服务端。当前语音部署模型中，一个 Runtime
进程对应一个 Agent RTC Session，避免不同会话共享可变播放或生成状态。

## 把核心对象连成一条链

可以把 Muxiva 想成一座受控的实时工厂：

| 概念 | 通俗解释 | 在系统中的职责 |
| --- | --- | --- |
| **Frame** | 流水线上的货物 | 不可变的数据单元，承载音频、视频、文本、字节等 Payload 与追踪 Header |
| **Node** | 加工机器 | 在生命周期回调中消费 Frame，并通过 `NodeContext` 发出零到多个输出或控制消息 |
| **Port** | 有规格的插座 | 以名称、方向、Frame Type 和 Schema 约束 Node 的输入输出 |
| **Edge** | 有容量的传送带 | 把一个输出 Port 接到一个输入 Port，并定义队列容量、背压与溢出策略 |
| **Graph** | 工厂设计图 | 声明 Node、Edge、配置和静态 DAG 拓扑，不保存运行状态 |
| **Manifest** | 机器说明书 | 描述 Node 身份、版本、语言、能力、配置和 I/O Schema |
| **Factory** | 机器生产线 | 根据 Manifest 和配置为每个 Graph Node ID 创建独立实例 |
| **Registry** | 可用机器目录 | 发现 Factory，并为 Graph 选择精确的类型、语言与版本 |
| **Runtime** | 工厂控制系统 | 负责物化、启动、调度、背压、取消、停止和故障收敛 |
| **NodeContext** | Node 的受控操作面板 | 提供具名输出、Signal、Event、取消和运行上下文，而不是直接调用下游 Node |

这条关系可以压缩为：

```text
Manifest + Factory → Registry → Graph Compiler → Runtime
                                           │
                     Frame → Node.output Port → bounded Edge → Node.input Port
```

## 一张 Graph 是怎样跑起来的

1. 开发者通过 SDK、JSON Graph v1 或 Studio 定义 Node 与 Edge。
2. `muxiva validate` / Graph Compiler 检查 Node 身份、配置 Schema、Port、Frame Type、
   Queue Policy 和 DAG 拓扑；此时不会启动 Node 或连接外部服务。
3. Registry 为每个 Graph Node ID 选择精确 Factory，Runtime 创建独立 Node 实例。
4. Runtime 调用所有 Node 的 `on_prepare`，然后启动 Source、Worker 和有界 Edge Queue。
5. Node 在 `on_process(frame, ctx)` 中通过 `ctx.emit(port, frame)` 发出结果；一个调用可以
   不输出，也可以从多个具名 Port 输出。
6. 正常结束进入 `on_finish`；错误、取消或超时进入 `on_abort`，Runtime 有界等待外部
   执行域与回调停止。

## 数据、控制与观测为什么要分开

实时 Agent 同时存在三类通信，它们不能混成一条“万能消息总线”：

| 通道 | 如何传播 | 应该承载什么 | 不应该承担什么 |
| --- | --- | --- | --- |
| **Frame + Edge** | 沿显式 Graph 拓扑和有界队列 | 音频、视频、ASR 文本、LLM 输出、客户端交互消息 | 全局广播 |
| **Signal** | 由 Runtime 沿当前 Node 的相邻 Edge 投递 | 打断、取消、清空旧播放等需要改变相关 Node 状态的控制 | 远程客户端传输 |
| **NotificationBus 通知** | 发布给进程内观察者 | 日志、指标、Studio 诊断、转写完成等可观测信息 | 浏览器协议或业务数据流 |

以 Barge-in 为例：VAD 识别到用户重新说话时只发出观察 Event；ASR 的最终文本进入
`builtin.voice_turn_controller`，由它过滤口水词、批准新轮次并唯一发出
`muxiva.turn.cancelled` Signal。Runtime 将该 Signal 投递给相关模型与播放 Node；模型取消
旧生成并丢弃晚到片段，播放 Node 清空陈旧音频。若需要把“用户正在说话”展示到远程 Voice Room，应由 Transport Node 把
客户端事件作为 Frame / 字节协议发送，而不是让浏览器访问 NotificationBus。

## 当前语音打断机制

![Muxiva 全双工语音打断时序](assets/control/muxiva-barge-in.drawio.png)

[下载可编辑的 Draw.io 源文件](assets/control/muxiva-barge-in.drawio)

这张图对应当前 `Qwen Realtime + Agora RTC` Graph 的真实执行路径：

1. 浏览器的麦克风上行不会因为 Agent 正在播放回答而停止，因此链路具备全双工输入条件。
2. Qwen Server VAD 报告 `input_audio_buffer.speech_started`，ASR Node 将它作为观察 Event
   输出，不直接取消任何工作。
3. 最终 Transcript 通过 Voice Turn Controller 的准入策略后，控制器发出唯一的
   `muxiva.turn.cancelled` Signal 和同 Sequence Prompt；Runtime 只沿显式 Signal Edge 投递。
4. Agent/TTS 丢弃旧 generation；Audio Sink 清空尚未发出的 PCM 队列、推进 sequence 取消水位，并拒绝不高于该水位的
   旧音频 Frame。这与 Qwen Node 的晚到分片过滤形成双保险。
5. `speech.started` 仍以观察 Event Frame 离开 Qwen；项目级 Voice Room Encoder
   把这些 Event 以及转写/回答 Text Frame 映射为应用协议，再由 Agora Data Sink 发到远程
   Voice Room；`publish_notification` 只进入进程内 NotificationBus，供日志、指标和
   Studio 诊断使用。

需要注意物理边界：已经进入 Agora 网络或浏览器播放器缓冲区的音频无法撤回。因此真正
低延迟的打断不仅依赖 Signal，还依赖 Audio Sink 以短 PCM 包发送、限制队列长度，并让
客户端避免过深的播放缓冲。Core 不切换业务 Turn，也不包含 Qwen 或 Agora 的打断策略。

## 用语音链路验证这套模型

```text
浏览器麦克风
  → Agora RTC 网络
  → C++ Agora Audio Source Node
  → Audio Resampler / Qwen ASR
  → 可选 Pi TypeScript Agent / Qwen TTS
  → 文本与音频 Frame
  → C++ Agora Data / Audio Sink Node
  → 浏览器聊天气泡与扬声器
```

这条链路里，Agora 不知道 Graph 如何调度，Qwen 不知道浏览器如何采集，浏览器拿不到
模型密钥，Rust Core 也不包含任何厂商业务代码。各层只通过 Manifest、Frame、Port、
Edge、Signal 和生命周期契约协作。

## 必须记住的架构边界

1. **Graph 是声明，Runtime 才是运行实例。** JSON 中不能放可执行代码或密钥。
2. **业务能力都是 Node。** “Provider”可以是文档分类，但不是新的运行时抽象。
3. **NotificationBus 是进程内观测面。** 跨机器消息必须经过 Transport Node 或应用协议。
4. **Core 不理解厂商与语音业务。** Turn、Barge-in、ASR、TTS 策略留在 Node。
5. **Studio 是本地开发面，不是生产客户端。** 项目 Web 与 Runtime 可以部署在不同机器。
6. **队列与关闭必须有界。** 实时系统不能靠无限缓存或无限等待隐藏故障。

## 推荐阅读顺序

1. [Rust Core 与核心对象](core-runtime.md)：深入 Frame、Node、Port、Edge、Graph；
2. [Graph 与类型化 Port](graph.md)：读写 Graph v1；
3. [实时流控与控制消息](realtime-control.md)：理解背压、Signal、NotificationBus 与打断；
4. [Node 扩展机制](extensibility.md)、[多语言执行](languages.md)与
   [TypeScript Agent Node](nodes/pi-agent.md)；
5. [统一 Node 架构](provider-architecture.md)；
6. [CLI、Studio 与项目 Web](developer-surfaces.md)；
7. [端到端语音链路](voice-architecture.md)与[可运行语音 Demo](voice-demo.md)。
