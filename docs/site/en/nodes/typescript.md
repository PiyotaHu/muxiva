# TypeScript Nodes

Studio can create and register a `node.ts` project package today.

```typescript
import type { GraphNodeImplementation } from '@muxiva/core'

export const node: GraphNodeImplementation = {
  onProcess(frame, ctx) {
    ctx.emit('text_out', { ...frame, text: frame.text.toUpperCase() })
    ctx.publishNotification('example.text.uppercased', { sequence: frame.sequence })
  },
}
```

The Worker context exposes `emit`, `emitSignal`, and `publishNotification`. Return
values are retained as compatibility sugar, but explicit emission supports
multiple actions during one lifecycle callback.

The standalone `@muxiva/core` SDK executes hosted TypeScript Nodes inside a
dedicated Worker. Values crossing the boundary must be structured-clone
compatible, and V1 callbacks are synchronous.

## Current Studio boundary

Studio project-package execution is not active yet. Saving makes the package
discoverable but does not permit it in a runnable Graph. The planned Host must:

1. resolve a locked `@muxiva/core` dependency;
2. type-check the package;
3. load the exact exported entrypoint in a Worker;
4. enforce lifecycle, cancellation, payload, and shutdown limits;
5. return structured diagnostics to Studio.
