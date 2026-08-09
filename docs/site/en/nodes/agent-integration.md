# Integrate an existing Agent

Real teams usually do not start by writing a Muxiva Node. They already own an
Agent with models, sessions, prompts, tools, knowledge, and a release process.
Muxiva should place that Agent into a real-time multimodal pipeline without
forcing the team to rewrite it.

This chapter is the complete SOP from an independent Agent repository to a
running Muxiva Graph.

## Three ownership layers

| Layer | Owner | Contains | Must not contain |
| --- | --- | --- | --- |
| Agent repository | application team | model harness, sessions, tools, policy, tests | Graph scheduling, RTC, ASR, TTS |
| Agent Node adapter | Agent project | Port mapping, configuration schema, Driver assembly | substantial business logic or vendor SDKs |
| Muxiva Runtime | Muxiva | Frames, Graph, queues, Signals, scheduling, Hosts, observability | Pi, Qwen, or application tools |

Replacing Pi, LangGraph, or an in-house Agent does not rewrite RTC, ASR, TTS,
or the Graph. Replacing Agora or Qwen Nodes does not affect the Agent repo.

## Stable AgentDriver interface

TypeScript Agents implement the `@muxiva/agent` Driver shape:

```typescript
interface AgentDriver {
  run(
    prompt: { text: string; sequence: number },
    sink: {
      text(delta: string): void
      event(type: string, payload?: Record<string, unknown>): void
    },
    signal: AbortSignal,
  ): Promise<void>

  cancel?(reason: unknown): void
  close?(): void | Promise<void>
}
```

`run` receives a committed ASR prompt. `sink.text` produces streaming Text
Frames; `sink.event` produces Agent, Turn, and Tool lifecycle Events.
`AbortSignal` is the primary cancellation path for barge-in and superseding
prompts. `close` releases sessions and subscriptions at Runtime shutdown.

This is not an HTTP protocol and does not require the Agent and Runtime to live
in one repository. It is the smallest testable contract between an application
Agent and a Muxiva Node.

## Graph Port contract

| Port | Frame | Semantics |
| --- | --- | --- |
| `prompt_in` | Text input | ASR Final, chat input, or an upstream plan |
| `signal_in` | Signal input | `muxiva.agent.cancel`, barge-in, and equivalent cancellation |
| `text_out` | Text output | streaming chunks for TTS, UI, or downstream Agents |
| `event_out` | Event output | response, turn, tool, and failure lifecycle |

`defineAgentNode` owns the bounded output queue, generation IDs, stale-result
suppression, internal wakeups, cancellation, and shutdown. The Agent does not
need a Clock Node and should not embed WebSocket, RTC, or browser protocols.

## SOP 1: prepare the independent Agent repository

```text
my-company-agent/
├── package.json
├── package-lock.json
├── src/
│   ├── index.ts           # exports createMyAgentDriver
│   ├── tools/
│   └── permissions.ts
└── test/
```

Export a Driver factory:

```typescript
export function createMyAgentDriver({ config }) {
  return {
    async run(prompt, sink, signal) {
      const result = await myAgent.run(prompt.text, { signal })
      for await (const delta of result.textStream) sink.text(delta)
    },
    cancel() { myAgent.cancel() },
    async close() { await myAgent.close() },
  }
}
```

Type-check and test tools, cancellation, and permissions in that repository.
Release a reviewed Tag. Never make production setup follow external `main`;
the demo pins `v0.1.2`, and applications should pin a reviewed Tag or Commit.

## SOP 2: create a thin Node adapter

```text
.muxiva/nodes/my_agent/
├── muxiva.node.json
└── node.ts
```

```typescript
import { defineAgentNode } from '@muxiva/agent'
import { createMyAgentDriver } from '@my-company/my-agent'

export const MyAgentNode = defineAgentNode({
  createDriver: createMyAgentDriver,
})
```

If this file contains model calls, filesystem logic, or dozens of business
tools, the layers are mixed again. Move them back to the Agent repository.

The Manifest declares the stable `node_type`, exact `factory_version`, four
Ports, and configuration JSON Schema. Studio uses it for discovery; the
Runtime uses it for Graph validation and Factory resolution.

## SOP 3: pin and install the Agent

```bash
git clone --depth 1 --branch v1.2.3 \
  https://github.com/my-company/my-agent.git \
  .muxiva/agents/my-agent

npm ci --ignore-scripts
npm run check:typescript
```

Reference that reviewed checkout from the application lock file:

```json
{
  "dependencies": {
    "@muxiva/agent": "file:../../bindings/agent",
    "@my-company/my-agent": "file:.muxiva/agents/my-agent"
  }
}
```

The flagship demo automates this path:

```bash
./examples/voice-agent/setup.sh
```

Exercise the same path with your repository:

```bash
MUXIVA_PI_AGENT_REPOSITORY=https://github.com/my-company/my-agent.git \
MUXIVA_PI_AGENT_REF=v1.2.3 \
./examples/voice-agent/setup.sh
```

Setup stops on a non-Git target, a mismatched remote, or local modifications;
it does not overwrite application-owned code.

## SOP 4: grant file and coding authority

Filesystem capability must answer three questions: where can the Agent act,
which operations are allowed, and what resource limits apply?

The Pi reference Agent defaults to:

```json
{
  "workspace_root": ".muxiva/workspaces/pi-agent",
  "max_file_bytes": 262144,
  "max_search_files": 500
}
```

The TypeScript Host sets the Graph project as the process working directory
and injects absolute `MUXIVA_PROJECT_ROOT`. The Agent accepts only a relative
workspace and rejects:

- absolute paths and `..` traversal;
- symlinks escaping the workspace;
- `.env`, `.env.*`, `.git`, and `.ssh`;
- files and searches beyond configured limits;
- overwriting an existing file without explicit `overwrite=true`.

Default tools list directories, read files, search text, create/write files,
and perform exact text replacement. Shell, process execution, arbitrary
deletion, and project-external access are disabled.

Set `workspace_root` to `.` only after reviewing Agent code and model policy.
A safer workflow gives the Agent a dedicated workspace, reviews its diff, and
merges through a human or CI gate.

## SOP 5: connect the Graph and verify interruption

```text
ASR.transcript_out ──Text──> Agent.prompt_in
VAD.signal_out ──Signal────> Agent.signal_in
Agent.text_out ──Text──────> TTS.text_in
Agent.event_out ──Event────> application Event Encoder
```

On barge-in, the same Signal reaches Agent, TTS, and audio egress. The Agent
cancels the active Pi Turn; the generic adapter advances its generation ID and
drops late Text/Event output. The next ASR Final Text starts a new Turn.

Verification checklist:

1. `muxiva doctor --voice` reports external Agent source, locked dependencies,
   and workspace as Ready;
2. Studio Validate succeeds and the Agent appears in the Palette;
3. “List workspace files” creates `tool.started/completed` events in Observe;
4. “Create index.html” produces a real file in the workspace;
5. barge-in prevents old answer and Tool results from reaching TTS;
6. Semantic Trace shows Text, Event, and Signal grouped by Turn;
7. `runtime.log` contains no Host framing or unexpected permission error.

## Reference implementation

[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent) is an
independent, forkable Pi coding Agent. The Muxiva demo keeps only a thin adapter
and pulls a pinned release from `setup.sh`. This is the recommended end-user
path: own and test an Agent first, then deploy a reviewed version into Muxiva.

See [Pi coding Agent](pi-agent.md) for its tools, configuration, and demo flow.
