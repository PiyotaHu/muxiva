# TypeScript SDK

## Install

Published releases will use:

```bash
pnpm add @voxa/core
```

Repository builds require Node.js 20–24, pnpm, Rust, and a supported native
toolchain. Run `pnpm install && pnpm build` in `bindings/node`.

## Develop a Node

```ts
import { NodeRunner, defineTransformNode } from '@voxa/core'

type Text = { kind: 'text'; text: string; sequence: number }
const uppercase = defineTransformNode<Text, Text>({
  onProcess(frame) { return { ...frame, text: frame.text.toUpperCase() } },
})

const runner = new NodeRunner(uppercase)
const output = await runner.process({ kind: 'text', text: 'hello', sequence: 1 })
await runner.finish()
await runner.close()
```

The implementation runs in a dedicated Worker with bounded admission. Values
must be structured-clone-compatible and JSON-serializable. V1 callbacks are
synchronous and self-contained: returning a Promise produces
`VOXA_NODE_PROMISE_UNSUPPORTED`, and callback source cannot capture lexical
variables from the caller. The complete strict-TypeScript consumer is in
`examples/typescript`.

## Register a Graph v1 Factory

```ts
const factory = new GraphNodeFactory('example.typescript.uppercase', {
  onProcess(frame: { text: string }) {
    return { text: frame.text.toUpperCase() }
  },
})
const workerTotal = await runGraph(graphJson, [factory])
```

`runGraph` creates a dedicated Worker. Rust compiles and runs the concurrent
Graph on a background task while lifecycle calls are scheduled back onto that
Worker's JavaScript event loop. Promise/thenable callbacks remain rejected in
V1. See `examples/typescript/registered-graph.ts`.

D04 v1 supports text Transform factories with empty `node_config`. Graph JSON
never evaluates or imports JavaScript by itself.
