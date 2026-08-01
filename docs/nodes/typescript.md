# TypeScript Node development

Use **Studio → Create Node → TypeScript** to create `node.ts` and a project
Manifest. The published SDK's hosted TypeScript Nodes run in a dedicated
Worker and must return structured-clone-compatible, synchronous values in V1.

```ts
import type { GraphNodeImplementation } from '@voxa/core'

export const node: GraphNodeImplementation = {
  onProcess(frame) {
    return { text_out: { ...frame, text: frame.text.toUpperCase() } }
  },
}
```

Studio project-package execution is **not active yet**. Saving registers the
package for authoring and discovery, but Studio keeps it out of runnable Graphs
until the planned Host can resolve `@voxa/core`, type-check the package, and
load the exact exported entrypoint. Use the programmatic SDK path documented in
the [TypeScript SDK reference](../sdk/typescript.md) meanwhile.

## 中文

Studio 会生成 `node.ts` 与 Manifest，但项目包执行 Host 尚未接通，因此只会保存
和展示，不会允许加入可运行 Graph。现有 TypeScript SDK 的 Node 在独立 Worker
运行；V1 暂不接受返回 Promise 的回调。
