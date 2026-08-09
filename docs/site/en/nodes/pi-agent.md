# TypeScript Agent Nodes and Pi

Demo 2 replaces a single-purpose LLM Node with a stateful TypeScript Agent
Node powered by [Pi](https://github.com/earendil-works/pi). Pi remains an
optional project dependency; Rust Core does not import Pi, Qwen, or any Agent
business logic.

## The reusable contract

`@muxiva/agent` turns a vendor-specific driver into the same Graph surface:

| Port | Type | Meaning |
| --- | --- | --- |
| `prompt_in` | Text input | A completed user prompt or assembled turn context |
| `tick_in` | Event input | Gives the Node bounded opportunities to drain background output |
| `signal_in` | Signal input | Cancels the active run on `muxiva.agent.cancel` or barge-in |
| `text_out` | Text output | Speech-sized assistant text chunks for TTS and UI |
| `event_out` | Event output | Agent, turn, Tool Call, completion, cancellation, and failure lifecycle |

The adapter owns output bounds, generation IDs, stale-output suppression,
shutdown, and cancellation. A driver owns only its model harness, transcript,
and tools:

```typescript
import { defineAgentNode } from '@muxiva/agent'

export const MyAgentNode = defineAgentNode({
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

This is the extension point for a Pi agent, another TypeScript agent harness,
or an application-owned implementation. The Graph and downstream Nodes do not
change when the driver changes.

## Demo 2's Pi implementation

The editable implementation is
`examples/voice-agent/.muxiva/nodes/pi_agent/node.ts`. It uses:

- `@earendil-works/pi-agent-core@0.84.1` for session state, streaming events,
  Tool Calls, and abort;
- `@earendil-works/pi-ai@0.84.1` with a custom OpenAI-compatible DashScope
  model definition;
- Qwen `qwen-flash` as the model, using the same Model Studio connection as
  ASR and TTS;
- safe example tools for current time and live weather. Weather data is
  provided by [Open-Meteo](https://open-meteo.com/).

Pi's coding tools are deliberately not enabled. A voice assistant should not
inherit shell, file-edit, or arbitrary process authority merely because the
underlying library can support them.

## Install and verify

Run the normal voice setup. It requires Node.js 22.19 or newer, installs the
locked npm graph without lifecycle scripts, and type-checks the Node:

```bash
./examples/voice-agent/setup.sh
muxiva doctor --voice
```

Doctor must report `pi-typescript-agent ... dependencies=locked`. In Studio,
choose **Pi Agent Full-Duplex Cascade (Demo 2)**. Ask for the current time or
today's weather to exercise a real Tool Call. The Runtime panel exposes
`muxiva.agent.tool.*` and `muxiva.agent.response.*` events without coupling the
Agent Node to the browser wire protocol.

## Full-duplex cancellation

When Qwen Server VAD detects new speech, its Signal reaches the Pi Agent, TTS,
the text cancellation gate, and Agora audio egress in parallel. The Pi driver
calls `agent.abort()`, the generic adapter advances its generation ID and drops
late chunks, and the next final ASR transcript becomes a new prompt in the same
Agent session.

The project-local Voice Room encoder maps generic Agent completion events to
the application's `muxiva.voice.*` protocol. That mapping does not belong in
Pi, the TypeScript Host, or Rust Core.
