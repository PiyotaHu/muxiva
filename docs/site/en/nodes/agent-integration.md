# Integrate an existing Agent

Real teams usually do not start by writing a Muxiva Node. They already own an
Agent with models, sessions, prompts, tools, knowledge, and a release process.
Muxiva should place that Agent into a real-time multimodal pipeline without
forcing the team to rewrite it.

This chapter is the complete SOP from an independent Agent repository to a
running Muxiva Graph.

## Four ownership layers

| Layer | Owner | Contains | Must not contain |
| --- | --- | --- | --- |
| Agent repository | application team | model harness, sessions, capability catalog, route policy, tools, tests | Graph scheduling, RTC, ASR, TTS |
| Agent Node adapter | Agent project | Port mapping, configuration schema, Driver assembly | substantial business logic or vendor SDKs |
| `@muxiva/agent` binding | Muxiva | generic request execution, capability contract and route validation | Turn admission, news/weather/device intent rules, or concrete tools |
| Muxiva Core/Runtime | Muxiva | Frames, Graph, queues, Signals, scheduling, Hosts, observability | business Turn semantics, Pi, Qwen, or application tools |

Replacing Pi, LangGraph, or an in-house Agent does not rewrite RTC, ASR, TTS,
or the Graph. Replacing Agora or Qwen Nodes does not affect the Agent repo.

## Stable AgentDriver interface

TypeScript Agents implement the `@muxiva/agent` Driver shape:

```typescript
interface AgentDriver {
  capabilities?(): readonly AgentCapability[]
  route?(prompt: AgentPrompt): AgentRouteDecision
  run(
    prompt: { text: string; sequence: number; route?: AgentRouteDecision },
    sink: {
      text(delta: string): void
      event(type: string, payload?: Record<string, unknown>): void
    },
    signal: AbortSignal,
  ): Promise<void>

  cancel?(reason: unknown): void
  snapshot?(): unknown
  close?(): void | Promise<void>
}
```

`run` receives a prompt already admitted by its upstream source. `sink.text`
produces streaming Text Frames; `sink.event` produces Agent and Tool lifecycle
Events. `AbortSignal` is the primary explicit cancellation path.
`capabilities` declares the maximum authority available to this Driver;
`route` synchronously selects a bounded subset for one request. The binding
rejects routes that attempt to grant undeclared capabilities. `snapshot` optionally
preserves session state when a timed-out or wedged Driver is rotated. `close`
releases sessions and subscriptions at Runtime shutdown.

This is not an HTTP protocol and does not require the Agent and Runtime to live
in one repository. It is the smallest testable contract between an application
Agent and a Muxiva Node.

## Graph Port contract

| Port | Frame | Semantics |
| --- | --- | --- |
| `prompt_in` | Text input | ASR Final, chat input, or an upstream plan |
| `signal_in` | Signal input | explicit request cancellation; voice Graphs wire the Voice Turn Controller's canonical Signal |
| `text_out` | Text output | streaming chunks for TTS, UI, or downstream Agents |
| `event_out` | Event output | response, tool, route, and failure lifecycle |

`defineAgentNode` owns the bounded output queue, per-request sinks, stale-result
suppression after explicit cancellation, internal wakeups, and shutdown. The
Agent does not need a Clock Node and should not embed WebSocket, RTC, or browser
protocols.

## Agent Node adapter and capability routing

`AgentNodeAdapter` is the concrete binding component returned by
`defineAgentNode`. It is not an extra Graph Node and it is intentionally not in
Rust Core. Core transports typed Frames and Signals; the adapter applies only
generic request-execution mechanics:

- input-order execution, bounded queues, explicit cancellation, and stale-output
  suppression after cancellation or Driver retirement;
- first-output and whole-request watchdogs, optional application-configured progress output, visible failure events, and
  Driver rotation with optional state transfer;
- capability declaration validation, per-request route validation, and
  `muxiva.agent.route.selected` observability.

It never interprets Turn IDs, Signal/Prompt sequence ordering, or a new prompt
as supersession. In a voice Graph, `builtin.voice_turn_controller` alone owns
Turn admission and supersession and sends an explicit cancellation Signal.

`CapabilityRouter` is a declarative helper for Agent repositories. Muxiva owns
the catalog/route schema and validates least-authority decisions, but route
matchers remain application policy. For example, the framework knows what
`tool.web_search` means as an opaque capability ID; it does not contain a regex
for “latest news”. A voice assistant may map that intent to web search while a
coding Agent may expose a different route set and workspace tools.

```typescript
const router = new CapabilityRouter({
  capabilities: [
    { id: 'model.chat', kind: 'model' },
    { id: 'tool.web_search', kind: 'tool' },
  ],
  routes: [{
    id: 'live_information',
    capabilities: ['model.chat', 'tool.web_search'],
    requiredCapabilities: ['tool.web_search'],
    match: applicationOwnsThisMatcher,
  }],
  fallback: { id: 'fast_chat', capabilities: ['model.chat'] },
})
```

Granted capabilities are the request's maximum authority. Required capabilities
are a validated subset that the Driver must satisfy before committing an
answer. This distinction prevents a model from guessing a news or weather
answer merely because the correct Tool was available but not invoked.

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
the demo pins `v0.2.1`, and applications should pin a reviewed Tag or Commit.

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

## SOP 5: add observable web search

Web search is an Agent tool, not a Muxiva Runtime responsibility. The reference
Agent declares `web_search` in its independent repository. Pi decides whether
current information is needed, then the tool invokes Bailian's native
DashScope endpoint with `forced_search`. File tasks and ordinary conversation
do not silently incur search calls.

```text
User question → Pi selects web_search → Bailian turbo search
                                     → answer + sources + latency
                                     → Pi writes a cited answer
```

Tool input contract:

| Field | Required | Meaning |
| --- | --- | --- |
| `query` | yes | focused query, up to 2,000 characters |
| `freshness_days` | no | restrict results to roughly the last 1–365 days |
| `domains` | no | restrict search to at most ten domains |

Output includes `answer`, `sources[]`, `model`, `duration_ms`,
`search_strategy`, and token usage. Each source preserves its original index,
title, site, and URL for citation. Muxiva does not parse Bailian protocol; it
observes the call through generic `tool.started/completed` Events and Semantic
Trace.

```json
{
  "web_search_enabled": true,
  "web_search_model": "qwen-flash",
  "web_search_strategy": "turbo",
  "web_search_max_sources": 10,
  "web_search_timeout_ms": 20000
}
```

Search reuses `DASHSCOPE_API_KEY` and `DASHSCOPE_WORKSPACE_ID`; it adds no third
credential. `turbo` is the low-latency voice default. Use `max` only when deeper
retrieval justifies additional latency and cost. The account must have Model
Studio search enabled, and Alibaba Cloud bills search separately. See the
[official Bailian web-search documentation](https://help.aliyun.com/zh/model-studio/web-search/).

## SOP 6: connect the Graph and verify interruption

```text
ASR.transcript_out ──Text──> VoiceTurnController.transcript_in
VAD.speech_out ─────Event─> VoiceTurnController.activity_in
VoiceTurnController.prompt_out ──Text──> Agent.prompt_in
VoiceTurnController.signal_out ─Signal─> Agent/TTS/audio cancellation inputs
Agent.text_out ─────────────Text───────> TTS.text_in
Agent.event_out ────────────Event──────> application Event Encoder
```

On an admitted barge-in, the Voice Turn Controller sends the same canonical
Signal to Agent, TTS, and audio egress before forwarding the new prompt. The
Runtime enforces this Signal-before-Frame barrier. The Agent adapter cancels its
active Pi request, closes that request's sink, and drops late Text/Event output.
Only the Voice Turn Controller creates the new Turn; the Agent adapter does not
infer it.

Verification checklist:

1. `muxiva doctor --voice` reports external Agent source, locked dependencies,
   and workspace as Ready;
2. Studio Validate succeeds and the Agent appears in the Palette;
3. “List workspace files” creates `tool.started/completed` events in Observe;
4. “Create index.html” produces a real file in the workspace;
5. “Search for today's Qwen updates and cite sources” produces a `web_search`
   trace and URLs in the answer;
6. barge-in prevents old answer and Tool results from reaching TTS;
7. Semantic Trace shows Text, Event, and Signal grouped by Turn;
8. `runtime.log` contains no Host framing or unexpected permission error.

## Reference implementation

[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent) is an
independent, forkable Pi coding Agent. The Muxiva demo keeps only a thin adapter
and pulls a pinned release from `setup.sh`. This is the recommended end-user
path: own and test an Agent first, then deploy a reviewed version into Muxiva.

See [Pi coding Agent](pi-agent.md) for its tools, configuration, and demo flow.
