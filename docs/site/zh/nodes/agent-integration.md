# 把现有 Agent 集成到 Muxiva

现实中的起点通常不是“先写一个 Muxiva Node”，而是团队已经拥有 Agent：它有自己的
模型、会话、提示词、Tool、知识库和发布流程。Muxiva 要解决的是把这个 Agent 放进
实时多模态链路，而不是要求团队重写 Agent。

本章给出从独立 Agent 仓库到可运行 Muxiva Graph 的完整 SOP。

## 先理解四层边界

| 层 | 谁维护 | 应该包含什么 | 不应该包含什么 |
| --- | --- | --- | --- |
| Agent 仓库 | 应用团队 | 模型 Harness、会话、能力目录、路由策略、Tool、业务测试 | Graph 调度、RTC、ASR、TTS |
| Agent Node 适配器 | Agent 项目 | Port 映射、配置 Schema、Driver 装配 | 大量业务代码、厂商 SDK |
| `@muxiva/agent` Binding | Muxiva | 通用请求执行、能力契约与路由校验 | Turn 准入、新闻/天气/设备意图规则或具体 Tool |
| Muxiva Core/Runtime | Muxiva | Frame、Graph、Edge 队列、Signal、调度、Host、可观测性 | 业务 Turn 语义、Qwen、Pi 或业务 Tool |

因此，更换 Pi、LangGraph 或自研 Agent 时，RTC、ASR、TTS 和 Graph 不需要重写；更换
Agora 或 Qwen Node 时，Agent 仓库也不需要感知。

## 稳定 AgentDriver 接口

TypeScript Agent 通过 `@muxiva/agent` 的稳定 Driver 形状接入：

```typescript
interface AgentDriver {
  capabilities?(): readonly AgentCapability[]
  route?(prompt): AgentRouteDecision
  run(
    prompt: { text: string; sequence: number },
    sink: {
      text(delta: string): void
      event(type: string, payload?: Record<string, unknown>): void
    },
    signal: AbortSignal,
  ): Promise<void>

  cancel?(reason: unknown): void
  snapshot?(): unknown
  close?(): void | Promise<void>
}
```

- `run` 接收一条已经由上游准入的用户问题；
- `sink.text` 输出流式回答，Muxiva 会转为 Text Frame；
- `sink.event` 输出 Agent 和 Tool 生命周期 Event；
- `AbortSignal` 是首选的显式取消通道；
- `cancel` 用于同步通知 Agent Harness；
- `capabilities` 声明本 Driver 可以授予的模型、Tool 或资源能力；
- `route` 为当前 Prompt 同步选择能力子集，Muxiva 会拒绝任何未声明能力；
- `snapshot` 在 Driver 熔断重建时传递同实现的私有会话快照，Muxiva 不解析它；
- `close` 在 Runtime 结束时释放会话、连接和订阅。

这不是 HTTP 协议，也不要求 Agent 与 Runtime 在同一个实现仓库。它是应用 Agent 与
Muxiva Node 之间最小、可测试的进程内契约。

## AgentNodeAdapter 在哪一层

`AgentNodeAdapter` 是 `@muxiva/agent` 导出的框架组件，也是 `defineAgentNode` 背后的
默认实现。它不是新的 Graph Node，也不属于 Rust Runtime Core。应用仍然只在 Graph 中
看到一个 Agent Node。

它只处理按输入顺序执行、输出队列、显式取消、首字与整次请求超时、取消后晚到结果抑制、
Driver 熔断重建和终止事件。它不读取 Turn ID，不比较 Signal/Prompt 序列，也不会把新 Prompt
解释成对旧请求的覆盖。语音 Graph 中只有 `builtin.voice_turn_controller` 负责 Turn 准入与
覆盖，并通过明确的 Signal 通知 Agent。Rust Core 仍只处理 Frame、Edge、Signal 和 Node
生命周期。

能力路由同样分两部分：Muxiva 定义并校验 `AgentCapability`、`AgentRouteDecision`，应用
Agent 决定“什么输入需要什么能力”。例如“最近新闻需要联网”属于 Pi Agent 的产品策略，
绝不会写入 Muxiva Core。每次有效决定都会产生 `muxiva.agent.route.selected` Event。

路由里的 `capabilities` 表示本次请求“最多允许使用”的权限；`requiredCapabilities` 是其中
“回答前必须真正满足”的子集。框架会校验必需能力没有越权，Driver 负责执行并在无法满足
时显式失败。这样新闻或天气问题不会因为 Tool 只是可用但没有调用，就把模型猜测提交给
UI/TTS。

## Graph Port 契约

推荐所有文本 Agent 使用同一组 Port：

| Port | Frame | 语义 |
| --- | --- | --- |
| `prompt_in` | Text 输入 | ASR Final、聊天输入或上游规划结果 |
| `signal_in` | Signal 输入 | 显式请求取消；语音 Graph 只接 Voice Turn Controller 的标准 Signal |
| `text_out` | Text 输出 | 可被 TTS、UI 或下游 Agent 消费的流式片段 |
| `event_out` | Event 输出 | response、tool、route、failure 生命周期 |

`defineAgentNode` 负责有界输出队列、每次请求的 Sink、显式取消后的晚到结果抑制、内部唤醒和
关闭。Agent 不需要在 Graph 中添加 Clock Node，也不需要把 WebSocket 或 RTC 逻辑放进
自己的代码。

## SOP 1：整理独立 Agent 仓库

Agent 仓库至少应做到：

```text
my-company-agent/
├── package.json
├── package-lock.json
├── src/
│   ├── index.ts           # 导出 createMyAgentDriver
│   ├── tools/             # 文件、搜索、业务 API 等 Tool
│   └── permissions.ts     # 权限与资源上限
└── test/
```

导出一个工厂函数：

```typescript
export function createMyAgentDriver({ config }) {
  return {
    async run(prompt, sink, signal) {
      const result = await myAgent.run(prompt.text, { signal })
      for await (const delta of result.textStream) sink.text(delta)
    },
    cancel() { myAgent.cancel() },
    async close() { await myAgent.close() },
  }
}
```

先在该仓库独立完成类型检查、Tool 单元测试、取消测试和权限测试，再发布 Tag。不要让
Muxiva 的 `setup.sh` 永远跟随外部仓库 `main`；Demo 使用 `v0.2.1`，真实项目也应该锁定
经过审查的 Tag 或 Commit。

## SOP 2：建立薄 Node 适配器

在 Agent 项目而不是 Agent 源码仓库中创建：

```text
.muxiva/nodes/my_agent/
├── muxiva.node.json
└── node.ts
```

`node.ts` 只做装配：

```typescript
import { defineAgentNode } from '@muxiva/agent'
import { createMyAgentDriver } from '@my-company/my-agent'

export const MyAgentNode = defineAgentNode({
  createDriver: createMyAgentDriver,
})
```

如果这里出现模型请求、文件操作或几十个业务 Tool，说明分层又混在了一起；这些代码应
回到独立 Agent 仓库。

Manifest 声明 `node_type`、精确 `factory_version`、四个 Port 和配置 JSON Schema。
Studio 使用 Manifest 展示 Node，Runtime 使用它做 Graph 校验和 Factory 解析。

## SOP 3：锁定并安装 Agent

参考 Demo 的安装方式：

```bash
git clone --depth 1 --branch v1.2.3 \
  https://github.com/my-company/my-agent.git \
  .muxiva/agents/my-agent

npm ci --ignore-scripts
npm run check:typescript
```

`package.json` 使用本地锁定仓库：

```json
{
  "dependencies": {
    "@muxiva/agent": "file:../../bindings/agent",
    "@my-company/my-agent": "file:.muxiva/agents/my-agent"
  }
}
```

旗舰 Demo 已把这一步自动化：

```bash
./examples/voice-agent/setup.sh
```

使用自己的 Agent 仓库验证相同集成路径：

```bash
MUXIVA_PI_AGENT_REPOSITORY=https://github.com/my-company/my-agent.git \
MUXIVA_PI_AGENT_REF=v1.2.3 \
./examples/voice-agent/setup.sh
```

脚本遇到已有的非 Git 目录、不同 Remote 或未提交修改时会停止，不会覆盖开发者代码。

## SOP 4：授予文件与编码权限

“Agent 能使用文件”必须同时回答三个问题：能访问哪里、能做什么、资源上限是多少。

Pi 参考 Agent 默认配置：

```json
{
  "workspace_root": ".muxiva/workspaces/pi-agent",
  "max_file_bytes": 262144,
  "max_search_files": 500
}
```

Muxiva TypeScript Host 会把 Graph 所在目录设置为进程工作目录，并注入绝对的
`MUXIVA_PROJECT_ROOT`。Agent 只接受相对 `workspace_root`，并拒绝：

- `..` 路径穿越和绝对路径；
- 指向工作区外部的符号链接；
- `.env`、`.env.*`、`.git` 和 `.ssh`；
- 超过配置上限的文件和搜索；
- 未显式 `overwrite=true` 的已有文件覆盖。

默认 Tool 是：目录浏览、文件读取、文本搜索、创建/写入、精确文本替换。默认没有
Shell、进程执行、任意删除和项目外权限。

若确实希望 Agent 处理整个项目，可以把 `workspace_root` 配为 `.`，但应该先审查 Agent
代码和模型策略。更稳妥的方式是把需要修改的源码放进独立 workspace，验收 Diff 后再由
人或 CI 合并。

## SOP 5：增加可观察的联网搜索 Tool

联网搜索属于 Agent 的工具能力，不属于 Muxiva Runtime。参考 Agent 在独立仓库中声明
`web_search`；外层 Pi 判断是否需要实时信息，工具再通过百炼 DashScope 原生接口执行
`forced_search`。普通文件任务和闲聊不会偷偷产生搜索调用。

```text
用户问题 → Pi Agent 判断需要联网 → web_search Tool
                                    → 百炼 turbo search
                                    → answer + sources + latency
                                    → Pi 组织带来源回答
```

工具输入契约：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `query` | 是 | 聚焦的检索问题，最多 2000 字符 |
| `freshness_days` | 否 | 只关注最近 1–365 天 |
| `domains` | 否 | 最多 10 个域名，仅搜索指定站点 |

工具输出包含 `answer`、`sources[]`、`model`、`duration_ms`、`search_strategy` 和 Token
用量。`sources[]` 保留搜索结果原始序号、标题、站点和 URL，供 Agent 在回答中引用。
Muxiva 不解析百炼协议：它只通过通用 `tool.started/completed` Event 和 Semantic Trace
观察这次工具调用。

参考配置：

```json
{
  "web_search_enabled": true,
  "web_search_model": "qwen-flash",
  "web_search_strategy": "turbo",
  "web_search_max_sources": 10,
  "web_search_timeout_ms": 20000
}
```

搜索复用 `DASHSCOPE_API_KEY` 和 `DASHSCOPE_WORKSPACE_ID`，不增加第三份凭据。`turbo`
用于实时语音的低延迟路径；需要更深检索时可以改成 `max`，但延迟和费用会增加。百炼
搜索需要账号已开通对应服务，并会单独计费。实现依据[百炼联网搜索官方文档](https://help.aliyun.com/zh/model-studio/web-search/)。

## SOP 6：连接 Graph 并验证打断

典型级联链路：

```text
ASR.transcript_out ──Text──> VoiceTurnController.transcript_in
VAD.speech_out ─────Event─> VoiceTurnController.activity_in
VoiceTurnController.prompt_out ──Text──> Agent.prompt_in
VoiceTurnController.signal_out ─Signal─> Agent/TTS/音频出口取消输入
Agent.text_out ─────────────Text───────> TTS.text_in
Agent.event_out ────────────Event──────> 应用 Event Encoder
```

一次插话被准入后，Voice Turn Controller 先向 Agent、TTS 和音频出口发送同一个标准
Signal，再转发新 Prompt；Runtime 保证同次回调的 Signal 先于 Frame 到达下游队列。
Agent 适配器只取消当前 Pi 请求并关闭该请求的 Sink，丢弃取消后晚到的 Text/Event。
新 Turn 只由 Voice Turn Controller 创建，Agent 适配器不推断。

验证清单：

1. `muxiva doctor --voice` 显示外部 Agent 源码、锁定依赖和 workspace 均 Ready；
2. Studio Validate 通过，Palette 中能看到 Agent Node；
3. 询问“列一下工作区文件”，Observe 中出现 `tool.started/completed`；
4. 要求“创建一个 index.html”，文件真实出现在 workspace；
5. 询问“搜索今天的 Qwen 更新并给出来源”，Trace 出现 `web_search` 且回答包含 URL；
6. Agent 回答过程中插话，旧回答与旧 Tool 结果不再进入 TTS；
7. Observe 的 Semantic Trace 能按 Turn 查看 Text、Event 和 Signal；
8. `runtime.log` 中没有 Host 协议破坏、路径拒绝之外的异常。

## 参考实现

[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent) 是一份独立、可 Fork
的 Pi 编码 Agent。Muxiva Demo 只保存薄适配器，并在 `setup.sh` 中拉取其锁定版本。这正是
推荐给最终用户的路径：先拥有并测试自己的 Agent，再把一个被审查的版本部署到 Muxiva。

具体 Tool、配置和 Demo 体验见 [Pi 编码 Agent](pi-agent.md)。
