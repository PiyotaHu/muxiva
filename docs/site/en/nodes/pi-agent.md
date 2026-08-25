# Pi coding Agent reference

Demo 2 now consumes the independent
[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent)
repository. Agent business code is not copied into Rust Core. This models the
real path where a user owns, tests, and releases an Agent, then deploys a pinned
version into a Muxiva project.

Read the [Agent integration SOP](agent-integration.md) before adapting another
Agent. This page documents the Pi reference implementation.

## Where the code lives

| Content | Location | Owner |
| --- | --- | --- |
| Pi session, Qwen model, tools, file policy | `PiyotaHu/muxiva-pi-agent` | independent Agent repo |
| queue, generation, cancellation adapter | `@muxiva/agent` under `bindings/agent` | Muxiva SDK |
| assembly of those two layers | `.muxiva/nodes/pi_agent/node.ts` | demo project |
| Ports, configuration, Connection schema | `.muxiva/nodes/pi_agent/muxiva.node.json` | demo project |
| pinned checkout and dependency install | `examples/voice-agent/setup.sh` | demo deployment |

The project adapter stays deliberately small:

```typescript
import { defineAgentNode } from '@muxiva/agent'
import { createMuxivaPiAgentDriver } from '@piyotahu/muxiva-pi-agent'

export const PiAgentNode = defineAgentNode({
  createDriver: createMuxivaPiAgentDriver,
})
```

## Capabilities

Release `v0.2.1` provides:

| Tool | Behavior |
| --- | --- |
| `workspace_info` | report workspace, authority, and resource limits |
| `list_files` | list a directory, optionally recursively |
| `read_file` | read a UTF-8 file or line range |
| `search_files` | exact text search across a bounded file set |
| `write_file` | create files; overwrite requires explicit intent |
| `replace_in_file` | exact code replacement with expected match count |
| `web_search` | real Bailian web search with synthesis, titles, sites, and source URLs |
| `get_current_time` | current time in a requested time zone |
| `get_current_weather` | live Open-Meteo weather |

Demo 2 is therefore not merely conversational. Ask it to list the workspace,
read requirements and create `index.html`, perform a precise text edit, search
all TODO locations, or find today's Qwen changes with citations. Files are written to:

```text
examples/voice-agent/.muxiva/workspaces/pi-agent/
```

## Why Shell is disabled

Structured file editing and machine-wide command execution are different
authority levels. The reference Agent can complete web and source-file tasks,
but has no Shell, arbitrary process, arbitrary deletion, or workspace-external
access.

Paths resolve relative to the Graph project. The implementation checks path
traversal, symlink escape, sensitive paths, and resource bounds. `.env`,
`.env.*`, `.git`, and `.ssh` are always denied.

If a fork adds a Shell tool, command allowlists, working directory, timeout,
output limits, network policy, and human confirmation belong to that Agent
repository's security contract—not Muxiva Core.

## What setup does

```bash
./examples/voice-agent/setup.sh
```

Setup:

1. checks out `PiyotaHu/muxiva-pi-agent` release `v0.2.1`;
2. stores it at Git-ignored `.muxiva/agents/muxiva-pi-agent`;
3. installs `@muxiva/agent`, the external Agent, and Pi through the application
   lock file;
4. type-checks both the demo adapter and external Agent;
5. runs the external Agent's filesystem-policy tests;
6. creates the default workspace and continues building Qwen and Agora Nodes.

Setup prints repository, Tag, resolved Commit, workspace, and permissions. It
reuses an existing checkout only when the remote matches and the checkout is
clean; it does not overwrite local Agent edits.

## Use your own Agent fork

```bash
MUXIVA_PI_AGENT_REPOSITORY=https://github.com/your-org/your-agent.git \
MUXIVA_PI_AGENT_REF=v1.0.0 \
./examples/voice-agent/setup.sh
```

Keep the `createMuxivaPiAgentDriver` package export, or change the thin demo
adapter to import your factory. See [Agent integration](agent-integration.md)
for the general application-owned path.

## Run and accept

```bash
muxiva doctor --voice
./examples/voice-agent/run.sh --studio
```

Choose **Pi Agent Full-Duplex Cascade (Demo 2)** and open Voice Room. Ask:

> Create an `index.html` with a gradient background and the current time in the workspace.

Inspect the real result:

```bash
ls -la examples/voice-agent/.muxiva/workspaces/pi-agent
```

Observe should show Tool lifecycle in Semantic Trace, Agent Text flowing into
TTS, and barge-in Signal cancelling the active Pi request. Late output from the
old generation must not continue to play.

Then ask a question that requires the live web, such as “Search for today's
Qwen announcements and cite the sources.” Observe should show
`web_search` `tool.started/completed`; Tool details contain `duration_ms`,
`search_strategy`, `search_calls`, and structured `sources`. Search reuses the
Model Studio Key and Workspace ID from Connections, so it needs no third
credential, but Alibaba Cloud bills the search call.
