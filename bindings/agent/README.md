# @muxiva/agent

Vendor-neutral TypeScript contract for long-lived, streaming Agent Nodes.

The package separates two responsibilities:

- the Agent driver owns a model harness, conversation state, and tools;
- the Muxiva adapter owns Graph Ports, bounded output, lifecycle, cancellation,
  and stale-response suppression.

An Agent Node has the stable Ports `prompt_in`, `signal_in`, `text_out`, and
`event_out`. The adapter requests Runtime-managed internal wakeups for bounded
background output; application Graphs do not need a clock Node. Provider-specific
code implements only `run`, optional `cancel`, and optional `close`.

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

Follow the [Agent integration SOP](https://piyotahu.github.io/muxiva/nodes/agent-integration/)
to keep an application Agent in its own repository and deploy a pinned release
through a thin project Node adapter. The independently versioned
[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent) repository
is the workspace-scoped coding Agent used by the flagship cascade demo.
