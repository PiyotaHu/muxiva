# Provider

Provider 将 Voxa 连接到 RTC SDK、媒体库、模型 API、Transport 和设备。它们必须
位于 Runtime Core 之外，并保持有界入口、所有权、取消与关闭契约。

## Agora

仓库当前包含：

- In-memory Mock RTC 契约；
- 可选的 Agora C++ PCM16 Audio 与 I420 Video Adapter；
- 可选的 Python Audio Provider；
- 带凭据实房与长稳测试脚本。

真实凭据永远不能提交到仓库。生产认证仍需保留各平台实房证据、长时间测试、重连
与晚到回调故障结果，以及与 Release 对应的 SDK 兼容记录。

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
