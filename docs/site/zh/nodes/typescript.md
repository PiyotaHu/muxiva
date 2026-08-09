# TypeScript Node

Studio 当前可以创建并注册 `node.ts` 项目 Package。

```typescript
import type { GraphNodeImplementation } from '@muxiva/core'

export const node: GraphNodeImplementation = {
  onProcess(frame, ctx) {
    ctx.emit('text_out', { ...frame, text: frame.text.toUpperCase() })
    ctx.publishNotification('example.text.uppercased', { sequence: frame.sequence })
  },
}
```

Worker Context 提供 `emit`、`emitSignal`、`publishNotification` 与
`scheduleNextTick(delayMs)`。返回值仍作为兼容
写法保留；显式发送允许一次生命周期回调执行多个互不排斥的动作。

独立 `@muxiva/core` SDK 会在专用 Worker 中执行 Hosted TypeScript Node。Studio
中的项目 Node 则运行在受管理的 Node.js 子进程里：Runtime 加载真实模块、等待异步
生命周期、接收 Context 的显式输出，并在关闭时终止进程。

## Studio 运行契约

Node.js 22.19 或更高版本可用时，Studio 会激活 TypeScript Package。Host 会：

1. 从 `node.ts` 加载 Manifest 指定的精确导出；
2. 把 Manifest 配置和当前输入 Port 传给每个回调；
3. 支持返回 Promise 的五个生命周期回调；
4. 通过有界 JSON Lines 协议传递类型化 Frame 和 NotificationBus 发布；
5. 将 stdout 专用于 Host 协议，把业务日志经 stderr 写进 Studio 的 `runtime.log`。

长时间的厂商流不能一直阻塞 `onProcess`。Node 应启动后台请求，将结果放入有界队列，
再用 `scheduleNextTick` 请求 Runtime 内部回调排空；这样 `onSignal` 才能立刻取消请求，
图中也不需要 Clock Node 或 `tick_in` Port。可复用的
[`@muxiva/agent` 契约](pi-agent.md)已经为 Agent Node 实现了这套策略。
