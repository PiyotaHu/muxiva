# TypeScript Node

Studio 当前可以创建并注册 `node.ts` 项目 Package。

```typescript
import type { GraphNodeImplementation } from '@voxa/core'

export const node: GraphNodeImplementation = {
  onProcess(frame) {
    return { text_out: { ...frame, text: frame.text.toUpperCase() } }
  },
}
```

独立 `@voxa/core` SDK 会在专用 Worker 中执行 Hosted TypeScript Node。跨边界
数据必须兼容 Structured Clone，V1 回调保持同步。

## 当前 Studio 边界

Studio 项目 Package 执行尚未启用。保存后 Package 可以被发现，但不能进入可
运行 Graph。后续 Host 必须：

1. 解析锁定版本的 `@voxa/core`；
2. 对 Package 执行类型检查；
3. 在 Worker 中加载精确导出入口；
4. 强制生命周期、取消、Payload 与关闭上限；
5. 向 Studio 返回结构化诊断。
