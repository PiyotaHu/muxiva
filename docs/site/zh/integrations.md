# Provider

Provider 将 Voxa 连接到 RTC SDK、媒体库、模型 API、Transport 和设备。它们必须
位于 Runtime Core 之外，并保持有界入口、所有权、取消与关闭契约。

## Agora

仓库当前包含：

- In-memory Mock RTC 契约；
- 可选的 Agora C++ PCM16 Audio 与 I420 Video Adapter；
- 带凭据实房与长稳测试脚本。

真实凭据永远不能提交到仓库。生产认证仍需保留各平台实房证据、长时间测试、重连
与晚到回调故障结果，以及与 Release 对应的 SDK 兼容记录。

## 门面语音方案：Agora + Qwen

真实 Voice Playground 的架构使用 Agora Web SDK 承担浏览器麦克风采集与播放。
Voxa Bot 通过 Native Adapter 加入同一房间，接收每个用户的 PCM，并通过自定义
音频轨道回推生成的 PCM。

智能层首个 Profile 选择阿里云百炼 Qwen Audio Realtime。一条服务端 WebSocket
提供轮次检测、语音识别、推理、流式语音与打断事件；类型化 Frame、`TurnId`、
取消、旧输出过滤、有界队列、路由和指标仍由 Voxa 管理。浏览器永远不能获得
DashScope API Key 或 Agora App Certificate。

确定性脚本图只用于 CI 模拟，不能替代需要凭据的真实链路。仓库中的 D10 设计记录
定义了媒体格式、配置与打断契约。

Studio 提供 DashScope 与 Agora 的 **Connections** 配置界面。Secret 使用密码
输入框，提交后立即清空；初版只保留在本地 Studio 进程内存中，状态接口不回显，
Graph 也不会保存。Voice Graph Gallery 同时展示推荐的 7-Node 端到端 Realtime
拓扑，以及可检查、可替换的 11-Node VAD → ASR → LLM → TTS 级联拓扑。只有当
对应的精确 Provider Factory 全部安装后，Studio 才允许应用模板，避免生成无法
校验和运行的图。

Provider 代码现在严格归应用所有。Qwen Audio Realtime Node Pack 使用 Python，位于
`examples/voice-agent`；Agora Transport 使用 C++，位于 `providers/agora/cpp` 以及
应用自己的 C++ Node 中。Core、Graph Builtin 与 Studio 不包含任何 Qwen、DashScope
或 Agora 代码。根 CMake 工程也不声明 Agora Target；Provider 使用自己的独立
CMake 工程，并且只单向依赖 Voxa 公共 ABI。Studio 只从项目 Manifest 发现通用连接字段和图模板。Python Qwen
协议测试覆盖 PCM 发送、流式音频/文本与 `response.cancel`；C++ 门禁会编译 Agora
Node 与 Adapter。

## FFmpeg

可选媒体层提供流式音频重采样，以及 RGBA8、I420 等 Video Scale 与颜色转换。
FFmpeg 保持为可选 Provider 依赖，不属于 Core 强制依赖。

## Provider 验收清单

Provider Proposal 必须定义：

- 支持的 SDK 版本、平台、架构与 License；
- 输入输出 Frame Schema 与 Clock 行为；
- 回调线程与所有权转移；
- Queue/Byte 上限、背压与 Overflow 行为；
- 取消、重连、晚到回调与关闭行为；
- Mock、故障注入、实房与长稳测试策略；
- 指标、诊断、凭据与 Secret 处理方式。
