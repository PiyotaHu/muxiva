# @muxiva/agent

Vendor-neutral TypeScript contract for long-lived, streaming Agent Nodes.

The package separates two responsibilities:

- the Agent driver owns a model harness, conversation state, and tools;
- the Muxiva adapter owns Graph Ports, bounded output, lifecycle, cancellation,
  and stale-response suppression.

An Agent Node has the stable Ports `prompt_in`, `tick_in`, `signal_in`,
`text_out`, and `event_out`. Provider-specific code implements only `run`,
optional `cancel`, and optional `close`.

```js
import { defineAgentNode } from '@muxiva/agent'

export const AgentNode = defineAgentNode({
  createDriver() {
    return {
      async run(prompt, sink, signal) {
        sink.text(`You said: ${prompt.text}`)
        sink.event('tool.completed', { name: 'example' })
      },
      cancel() {},
    }
  },
})
```

The Runtime does not know which model harness is behind the driver. Pi, another
TypeScript agent library, or an application-owned implementation can all use
the same contract.
