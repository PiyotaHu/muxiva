# @muxiva/core

Build Muxiva Nodes in TypeScript or JavaScript. Each Node runs in a dedicated
`worker_threads` execution domain with bounded admission and structured errors.

Published releases will use:

```bash
pnpm add @muxiva/core
```

```ts
import { NodeRunner, defineTransformNode } from '@muxiva/core'

type Text = { kind: 'text'; text: string; sequence: number }

const node = defineTransformNode<Text, Text>({
  onProcess(frame) {
    return { ...frame, text: frame.text.toUpperCase() }
  },
})

const runner = new NodeRunner(node)
console.log(await runner.process({ kind: 'text', text: 'hello', sequence: 1 }))
await runner.finish()
await runner.close()
```

V1 lifecycle callbacks are synchronous. Returning a Promise or thenable raises
`MUXIVA_NODE_PROMISE_UNSUPPORTED`. Values crossing the Worker boundary must be
structured-clone-compatible and JSON-serializable. Callback methods must be
self-contained because their source is installed into the dedicated Worker.

See the [TypeScript SDK guide](../../docs/sdk/typescript.md) and the independent
[`examples/typescript`](../../examples/typescript) project.

`GraphNodeFactory` plus `runGraph` registers an exact-version TypeScript text
Transform into Graph v1. The complete graph runs in Rust while callbacks stay
on a dedicated Worker; Graph JSON never imports JavaScript code.
