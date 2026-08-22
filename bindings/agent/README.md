# @muxiva/agent

Vendor-neutral TypeScript contract for long-lived, streaming Agent Nodes.

The package separates three responsibilities:

- Rust Runtime Core owns Graph scheduling, Frames, bounded Edge queues, Signals,
  and Node lifecycle;
- framework `AgentTurnController` owns turn admission, bounded output,
  cancellation, deadlines, stale-response suppression, and Driver recovery;
- the Agent driver owns model/session state, capability policy, and tools.

An Agent Node has the stable Ports `prompt_in`, `signal_in`, `text_out`, and
`event_out`. The adapter requests Runtime-managed internal wakeups for bounded
background output; application Graphs do not need a clock Node. Provider-specific
code implements `run` plus optional `capabilities`, `route`, `cancel`, `snapshot`,
and `close`.

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

`defineAgentNode` constructs an exported `AgentTurnController`; applications do
not add another Graph Node for it. A Driver that declares capabilities can make
a synchronous route decision before each turn. Muxiva validates that the route
cannot grant undeclared capabilities and emits `muxiva.agent.route.selected`.

```js
import { CapabilityRouter } from '@muxiva/agent'

const router = new CapabilityRouter({
  capabilities: [
    { id: 'model.chat', kind: 'model' },
    { id: 'tool.web_search', kind: 'tool' },
  ],
  routes: [{
    id: 'live',
    capabilities: ['model.chat', 'tool.web_search'],
    requiredCapabilities: ['tool.web_search'],
    match: ({ text }) => applicationNeedsCurrentInformation(text),
  }],
  fallback: { id: 'fast', capabilities: ['model.chat'] },
})
```

Match functions are application policy. The framework contains no weather,
news, language, or Coding-Agent intent rules.

`capabilities` means “may use”; `requiredCapabilities` is its validated subset
meaning “must satisfy before committing an answer”. The Driver owns execution
and must fail visibly instead of silently answering when a required Tool cannot
run.

The Runtime does not know which model harness is behind the driver. Pi, another
TypeScript agent library, or an application-owned implementation can all use
the same contract.

Follow the [Agent integration SOP](https://piyotahu.github.io/muxiva/nodes/agent-integration/)
to keep an application Agent in its own repository and deploy a pinned release
through a thin project Node adapter. The independently versioned
[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent) repository
is the workspace-scoped coding Agent used by the flagship cascade demo.

Architecture details: [D11 Agent Turn Controller and Capability Routing](../../docs/design/d11-agent-turn-controller-capability-routing.md).
