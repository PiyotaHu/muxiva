# 测试体系

Voxa 将确定性的失败行为视为公开契约的一部分。

## 本地质量门禁

```bash
./scripts/check-quality.sh
```

独立检查覆盖 Rust、Python、Node.js、C ABI、C++ Consumer、RTC、Media、
Sanitizer、Benchmark、Fuzz、Miri 与 Studio 浏览器测试。

## 离线 Runtime 模拟器

`voxa demo` 和 `examples/graphs/mock-realtime-voice.v1.json` 是无网络、无凭据的
Runtime 测试夹具。它们使用合成 PCM 和预制文本验证分叉/汇合、背压、Signal、
EventBus、Turn 与生命周期，不提供真实 ASR、LLM 或 TTS，也不作为产品 Demo。

```bash
voxa demo --turns 4
voxa studio examples/graphs/mock-realtime-voice.v1.json
```

## 持续集成

受保护的 Pull Request 当前要求：

- Rust 格式、Clippy 零 Warning 与 Workspace 测试；
- Ubuntu 与 macOS 的 Python 测试；
- Ubuntu 与 macOS 的 Node.js 测试；
- Ubuntu 与 macOS 的 Native FFI、RTC 和 Media 检查；
- CODEOWNERS Review，并解决所有 Review 对话。

定时或附加 Workflow 继续覆盖 Sanitizer、Miri、Fuzz、Benchmark 与严格文档
构建。

## 测试必须证明什么

- Graph 与 Port 精确校验；
- Queue Capacity 与 Overflow 行为；
- 取消与幂等关闭；
- 晚到结果与晚到外语回调；
- ABI 边界的所有权与 Buffer 生命周期；
- 不在禁止的 RTC 或 Scheduler Thread 上执行工作；
- 稳定诊断与最终指标保留；
- 时间、内存、Byte 与在途任务都有上限。

!!! note
    报告 `SKIP` 不等于测试通过。缺失工具链或凭据必须在认证结果中清晰可见。
