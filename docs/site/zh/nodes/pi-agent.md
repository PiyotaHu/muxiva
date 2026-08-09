# TypeScript Agent Node 与 Pi

Demo 2 已将单一用途的 LLM Node 替换为有状态 TypeScript Agent Node，执行内核采用
[Pi](https://github.com/earendil-works/pi)。Pi 只是可选的项目依赖；Rust Core 不导入
Pi、Qwen 或任何 Agent 业务逻辑。

## 可复用 Agent 契约

`@muxiva/agent` 将厂商相关 Driver 统一为同一组 Graph Port：

| Port | 类型 | 含义 |
| --- | --- | --- |
| `prompt_in` | Text 输入 | 完整用户问题，或已经融合的 Turn Context |
| `tick_in` | Event 输入 | 让 Node 有界地排空后台流式结果 |
| `signal_in` | Signal 输入 | 收到 `muxiva.agent.cancel` 或用户插话时取消当前运行 |
| `text_out` | Text 输出 | 适合 TTS 和界面消费的流式回答片段 |
| `event_out` | Event 输出 | Agent、Turn、Tool Call、完成、取消和失败生命周期 |

通用适配器负责有界队列、Generation ID、过期输出抑制、关闭与取消；Driver 只负责
模型 Harness、会话记录和工具：

```typescript
import { defineAgentNode } from '@muxiva/agent'

export const MyAgentNode = defineAgentNode({
  createDriver() {
    return {
      async run(prompt, sink, signal) {
        sink.text(`你说了：${prompt.text}`)
        sink.event('tool.completed', { name: 'example' })
      },
      cancel() {},
    }
  },
})
```

开发者可以在这里接 Pi、其他 TypeScript Agent Harness，或自己实现的 Agent。替换
Driver 不会改变 Graph 和下游 Node。

## Demo 2 的 Pi 实现

可编辑源码位于 `examples/voice-agent/.muxiva/nodes/pi_agent/node.ts`，它使用：

- `@earendil-works/pi-agent-core@0.84.1`：会话状态、流式事件、Tool Call 和取消；
- `@earendil-works/pi-ai@0.84.1`：通过自定义 OpenAI 兼容模型连接百炼；
- Qwen `qwen-flash`：与 ASR、TTS 复用同一张百炼 Connection；
- 当前时间与实时天气两个安全示例工具。天气数据来自
  [Open-Meteo](https://open-meteo.com/)。

这里刻意没有启用 Pi Coding Agent 的 Shell、文件读取和编辑工具。底层库“能够做”
不等于语音助手“应该自动获得”这些权限。

## 安装与验证

沿用语音 Demo 的安装命令。它要求 Node.js 22.19 或更高版本，以禁用生命周期脚本的
方式安装锁定依赖，并执行严格 TypeScript 检查：

```bash
./examples/voice-agent/setup.sh
muxiva doctor --voice
```

Doctor 必须出现 `pi-typescript-agent ... dependencies=locked`。随后在 Studio 选择
**Pi Agent Full-Duplex Cascade（Demo 2）**，询问当前时间或今天的天气，即可触发
真实 Tool Call。Runtime 面板会显示 `muxiva.agent.tool.*` 与
`muxiva.agent.response.*`，Agent Node 不需要知道浏览器的消息协议。

## 全双工打断

Qwen Server VAD 检测到用户重新开口后，同一个 Signal 会并行到达 Pi Agent、TTS、
文本取消门和 Agora 音频出口。Pi Driver 调用 `agent.abort()`；通用适配器推进
Generation ID 并丢弃晚到片段；下一条 ASR Final Text 会在同一 Agent Session 中成为
新问题。

项目内的 Voice Room Encoder 最后才把通用 Agent 完成事件映射为应用自己的
`muxiva.voice.*` 协议。这个映射不属于 Pi、TypeScript Host 或 Rust Core。
