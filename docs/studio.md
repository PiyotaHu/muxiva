# Muxiva Studio

Muxiva Studio is a dependency-free local visual editor bundled into `muxiva-cli`.
It edits the same strict Graph v1 JSON document used by `muxiva validate` and the
runtime compiler—there is no separate browser-only graph format.

## Launch

```bash
muxiva studio examples/graphs/text-uppercase.v1.json
```

The CLI validates the graph, binds an ephemeral loopback port, prints a
tokenized URL, and opens the default browser. Use `--no-open` when running over
SSH or when you want to open the printed URL yourself:

```bash
muxiva studio graph.json --port 4173 --no-open
```

## Visual workflow

- Drag trusted built-in or project Nodes from the Palette onto the canvas.
- Drag an output port onto a compatible input port. Studio derives the Edge
  Frame type from the port schemas and rejects incompatible connections.
- Drag Nodes to arrange the local canvas.
- Select a Node to edit its ID, type, and JSON configuration.
- Add or remove typed edges and configure capacity/overflow policy.
- Use Undo/Redo or edit the complete Graph v1 source in the JSON drawer.
- Validate at any time; diagnostics point back to the affected Node.
- Save writes formatted Graph v1 JSON atomically to the file passed to the CLI.
- Run starts the current canvas as an isolated local runtime snapshot; saving is
  not required first.
- Stop requests the Runtime's idempotent bounded shutdown path.
- The runtime panel shows Node callback counts/duration, active/error Nodes,
  Edge queue pressure, throughput, drops, and the retained terminal outcome.

Canvas positions are presentation state and are intentionally not written into
Graph v1. Reopening Studio derives a deterministic layout from the graph.

## Create and register a project Node

Click **Create Node** in the Palette to open Node Lab. Pick a language, edit the
starter code, declare named typed ports and configuration JSON Schema, then
press **Save & Register**. Studio writes a package beside the Graph:

```text
.muxiva/nodes/my_python_node/
├── muxiva.node.json
└── node.py
```

The package appears in the current project Palette immediately. Text Python
Source, Transform, and Sink packages run through the local Python development
Host. TypeScript, Rust, and C++ packages are authorable now and remain disabled
on runnable Graphs until their corresponding build Host is available. See the
[Node authoring overview](nodes/README.md) and language guides for the exact
contracts.

## Security boundary

Studio defaults to `127.0.0.1`, serves no remote assets, enables no CORS, and
requires a random bearer token for every graph API. The token starts in the URL
fragment, is removed from browser history, and is retained only for the current
browser tab session. Responses use a strict Content Security Policy and refuse
documents larger than 1 MiB.

Binding a non-loopback address requires an explicit `--host` and prints a
warning. Studio is a local development tool, not a remotely hosted control
plane, and must not be exposed directly to the internet.

## Current boundary

The Palette merges the trusted built-in Registry with this Graph's project Node
Library. Node type, language, exact Factory version, ports, and configuration
schema therefore match Graph compilation. Studio loads project source only
when a trusted local user presses **Run**; saving and browsing never execute it.
Remote package discovery, signed dependencies, and production control remain
follow-up work.
