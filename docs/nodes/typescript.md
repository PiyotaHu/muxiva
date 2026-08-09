# TypeScript Node development

Use **Studio → Create Node → TypeScript** to create `node.ts` and a project
Manifest. The standalone SDK's hosted TypeScript Nodes run in a dedicated
Worker and keep synchronous callback semantics. Studio project Nodes use a
managed Node.js 22.19+ subprocess and may implement asynchronous lifecycle methods.

```ts
import type { GraphNodeImplementation } from '@muxiva/core'

export const node: GraphNodeImplementation = {
  onProcess(frame) {
    return { text_out: { ...frame, text: frame.text.toUpperCase() } }
  },
}
```

Studio loads the exact `node.ts` export, passes configuration and input-Port
context, streams explicit emissions over a bounded Host protocol, and turns
module/process failures into structured Runtime diagnostics. Long-running
provider work should use a background task plus `ctx.scheduleNextTick(delayMs)`
to drain bounded output without a Clock Node, so `onSignal` remains responsive. See the
[TypeScript SDK reference](../sdk/typescript.md).

## 中文

Studio 会生成 `node.ts` 与 Manifest，并通过 Node.js 22.19+ 的受管理子进程把项目
Package 注册到可运行 Graph。项目 Host 支持异步生命周期；独立 `@muxiva/core`
Worker SDK 仍保持同步回调契约。后台任务通过 `ctx.scheduleNextTick(delayMs)` 请求
Runtime 内部回调，不需要在 Graph 中增加 Clock Node。
