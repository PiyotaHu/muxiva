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

The Worker context exposes `emit`, `emitSignal`, `publishNotification`, and
`scheduleNextTick(delayMs)`. Return
values are retained as compatibility sugar, but explicit emission supports
multiple actions during one lifecycle callback.

The standalone `@muxiva/core` SDK executes hosted TypeScript Nodes inside a
dedicated Worker. Studio project Nodes run in a managed Node.js subprocess so
the Runtime can load the actual module, await asynchronous lifecycle methods,
stream explicit Context actions, and terminate the process on shutdown.

## Studio runtime contract

Studio activates a TypeScript package when Node.js 22.19 or newer is available.
The Host:

1. loads the exact exported entrypoint from `node.ts`;
2. passes Manifest configuration and the current input Port to every callback;
3. awaits all five lifecycle callbacks when they return Promises;
4. carries typed Frame emissions and NotificationBus publications over a
   bounded JSON-lines protocol;
5. reserves stdout for protocol messages and sends application diagnostics to
   Studio's `runtime.log` through stderr.

Long-running provider streams must not keep `onProcess` blocked. Start the
request in the background, buffer bounded results, and use `scheduleNextTick`
to request input-free Runtime callbacks. `onSignal` remains responsive and the
Graph needs no Clock Node or `tick_in` Port. The reusable
[`@muxiva/agent` contract](pi-agent.md) implements this policy for Agent Nodes.
