# 状态与路线图

Voxa 当前为 pre-alpha。基础阶段已经实现，但公开 Package 发布和多个执行 Host
尚未完成。

| 领域 | 状态 | 当前边界 |
| --- | --- | --- |
| Frame 与并发 Graph Runtime | 可用 | 静态 DAG 与精确类型化 Port |
| 背压与控制面 | 可用 | 有界队列、Signal、Event、Turn Control |
| C ABI 与 C++ SDK | 可用 | 版本化 ABI 与可安装 CMake Package |
| Python 与 Node.js SDK | 实验性 | 受管执行域与 Hosted Text Factory |
| Studio | 可用 | Node Lab、类型化连线、校验、Run/Stop、指标 |
| Studio Python 项目 Host | 实验性 | Text Source、Transform 与 Sink |
| Studio TypeScript/Rust/C++ Host | 规划中 | 当前仅支持编辑保存 |
| Agora 与 FFmpeg | 实验性 | Mock 与可选 Provider 路径 |
| Package Release | 规划中 | 尚无稳定公开版本 |

## 近期优先级

1. 完成多语言项目 Node 执行 Host。
2. 统一 CLI、Runtime 与 Studio 的项目 Registry 和 Lockfile 行为。
3. 发布带校验和与 Provenance 的独立 CLI、Python Wheel、npm Package 和 C++
   SDK 压缩包。
4. 用真实浏览器端到端测试替代 Studio `SKIP` 检查。
5. 增加 Coverage、安全审计、API/ABI 兼容与 Release Gate。
6. 为每个 Release 平台保留 Provider 实房认证证据。

Pre-alpha 的 Breaking Change 也必须记录到 Changelog，并提供迁移说明。
