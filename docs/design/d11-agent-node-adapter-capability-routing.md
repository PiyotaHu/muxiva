# D11 — Agent Node Adapter and Capability Routing

Status: **implemented**

Last updated: **2026-08-25**

## 1. Decision

Muxiva treats an Agent as a Node, not as a second runtime. Rust Core continues
to own Graph scheduling, typed Frames, bounded Edge queues, Signal delivery,
and Node lifecycle. It does not infer conversations, intent, tools, or stale
assistant output.

Reusable Agent request-execution mechanics live in the framework-owned
`@muxiva/agent` binding. Conversational Turn admission and supersession live
only in an explicit upstream controller such as `builtin.voice_turn_controller`.
Product capability policy and concrete tools live in the Agent repository.

```text
Rust Runtime Core
  Graph · Frame · Edge · Signal · lifecycle
                    │
                    ▼
@muxiva/agent
  AgentNodeAdapter · capability contract · route validation
                    │
                    ▼
Application AgentDriver
  capabilities() · route() · run() · cancel() · snapshot()
                    │
                    ▼
Concrete Agent / model / business tools
```

This preserves the Stage 6 rule: Core supplies control mechanisms; explicit
Nodes own conversational policy; bindings own reusable request mechanics.

## 2. AgentNodeAdapter

`AgentNodeAdapter` is the concrete implementation behind `defineAgentNode`. It
is a framework binding class, not a Rust Core object and not a new Graph
entity. Studio still sees one ordinary Agent Node with typed Ports.

It owns only vendor-neutral mechanics:

- sequential execution of prompts received on `prompt_in`;
- bounded output admission and bounded drain per wakeup;
- per-request sink rejection of output after explicit cancellation or Driver retirement;
- explicit cancellation from `signal_in` through `AbortSignal` plus optional Driver `cancel`;
- first-output and whole-request deadlines;
- progress and terminal failure output configured by the application;
- circuit breaking and Driver replacement when cancellation is ignored;
- optional synchronous Driver state snapshot during replacement;
- normalized response, tool, route, timeout, and recovery events;
- bounded Runtime shutdown even when a retired Driver never settles.

The adapter does not know whether the input came from ASR, HTTP, or another
Agent. It does not inspect Turn IDs or compare Signal and Prompt sequences. A
new prompt never implicitly cancels an active request. It does not know what
weather or news means and does not select a model.

An explicit Signal cancels only the request executing at that moment. Queued
prompts retain input order. When one upstream callback emits both a Signal and
a Frame, the Runtime's generic Signal-before-Frame barrier makes the causal
order deterministic without putting Turn policy in this adapter.

### Request execution states

```text
idle
  └─ prompt ─> queued ─> running ─> completed ─> idle
                              ├─ cancel ─> cancelled ─> idle
                              ├─ timeout ─> driver_rotated ─> failed ─> idle
                              └─ error ───> driver_rotated ─> failed ─> idle
```

A retired Driver may still finish in its own library. Its request sink is
closed, so it cannot affect the Graph or a later request.

## 3. Capability contract

An Adapter may declare a catalog:

```ts
interface AgentCapability {
  id: string
  kind: string
  description?: string
}
```

Examples are `model.chat`, `tool.web_search`, and `tool.read_file`. Identifiers
are opaque to Muxiva. The framework validates syntax and uniqueness only.

Before a request, an Adapter may synchronously return:

```ts
interface AgentRouteDecision {
  id: string
  capabilities: readonly string[]
  requiredCapabilities?: readonly string[]
  reason?: string
  metadata?: Readonly<Record<string, unknown>>
}
```

`capabilities` is the maximum authority granted for the request;
`requiredCapabilities` is the subset that the Driver must actually satisfy
before committing an answer. The adapter rejects a decision that references
an undeclared capability or requires a capability outside the granted set,
adds the validated decision to `AgentPrompt.route`, and emits
`muxiva.agent.route.selected`. A route function must be synchronous: it is a
request-routing decision, not another unbounded model call.

`CapabilityRouter` is an optional deterministic helper. Applications provide
the match functions and route profiles; Muxiva provides validation and a
stable decision shape. Muxiva never ships language-specific intent regexes.

## 4. Framework mechanism versus business policy

| Concern | Owner |
| --- | --- |
| Frame, Edge queue, Node lifecycle, and Signal-before-Frame callback barrier | Rust Runtime Core |
| Voice Turn admission, filler filtering, supersession, Turn ID and generation | `builtin.voice_turn_controller` |
| Request execution, explicit cancellation, deadline, stale suppression, Driver rotation | `@muxiva/agent` |
| Capability IDs and route decision contract | `@muxiva/agent` |
| “recent news needs web search” | application Agent repository |
| Weather, artwork, volume, file Tool implementation | application Agent repository |
| Which capabilities are enabled for one deployment | Graph Node configuration |
| ASR endpointing, activity observations, transcript previews and finals | ASR Node |
| TTS chunking and playback buffering | formatter, TTS, and Sink Nodes |

This prevents two opposite failures: putting product vocabulary in Core, or
making every Agent Adapter reinvent cancellation and queue correctness.

## 5. Adapter contract

```ts
interface AgentDriver {
  capabilities?(): readonly AgentCapability[]
  route?(prompt): AgentRouteDecision
  run(promptWithValidatedRoute, sink, signal): Promise<void>
  cancel?(reason): void
  snapshot?(): unknown
  close?(): void | Promise<void>
}
```

Older Drivers that implement only `run`, `cancel`, and `close` remain valid.
Drivers with capability routing receive the validated decision on the prompt.
`snapshot` is deliberately opaque: Muxiva transports it only to the same
Driver factory during recovery and does not inspect conversation state.

## 6. Reference policy in muxiva-pi-agent

The Pi reference Adapter declares model, information, device, artwork, web,
and optional workspace capabilities. Its product policy chooses profiles such
as:

- `chat.fast`: model only;
- `web.live_information`: model plus live search, with live search required;
- `weather.current` / `weather.forecast`: model plus one required weather Tool;
- `time.current`: model plus required current-time Tool;
- `device.volume` and `artwork.*`: narrow deterministic device capabilities;
- `coding.workspace`: model plus explicitly enabled workspace Tools.

The catalog is derived from the Tool instances actually created by the enabled
capability packs; configuration booleans are not a second source of truth. If
an utterance requires a disabled Tool, policy returns an explicit unavailable
route and the Driver fails the request. A required Tool is satisfied only by a
successful Tool completion event, never by a Tool call attempt or model text.

Voice presentation is outside the Agent contract. The Agent emits semantic text
deltas without sentence batching, stage-direction filtering, TTS formatting, or
emotion inference. A downstream speech formatter owns presentation-ready text;
transport-specific adapters may derive device animation from that formatted
text without adding voice policy to the Agent loop.

The policy is independently tested in the Pi repository. Replacing it with a
classifier, rules engine, or organization policy service does not change
Muxiva, provided it returns the same synchronous decision contract.

## 7. Observability and acceptance

Required semantic events are:

- `muxiva.agent.route.selected`;
- `muxiva.agent.response.started/completed/failed/cancelled`;
- `muxiva.agent.tool.started/updated/completed`;
- log events for `first_activity`, `first_text`, `request.timeout`, and
  `driver.rotated`.

Acceptance tests must prove:

1. a route cannot grant an undeclared capability;
2. a Driver that ignores explicit cancellation cannot block the next request;
3. late output from a retired Driver is suppressed;
4. first-output and request deadlines produce terminal output;
5. a Driver snapshot can be supplied to its replacement;
6. a restricted deployment cannot expose web or workspace Tools;
7. stable chat does not accidentally receive live-search capability.
