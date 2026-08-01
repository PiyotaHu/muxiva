# Voxa Studio

Voxa Studio is a dependency-free local visual editor bundled into `voxa-cli`.
It edits the same strict Graph v1 JSON document used by `voxa validate` and the
runtime compiler—there is no separate browser-only graph format.

## Launch

```bash
voxa studio examples/graphs/text-uppercase.v1.json
```

The CLI validates the graph, binds an ephemeral loopback port, prints a
tokenized URL, and opens the default browser. Use `--no-open` when running over
SSH or when you want to open the printed URL yourself:

```bash
voxa studio graph.json --port 4173 --no-open
```

## Visual workflow

- Add trusted built-in source, transform, and sink Nodes from the palette.
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

The palette is generated from the trusted runtime Registry returned by
`GET /api/v1/registry/nodes`. Node type, language, exact Factory version,
ports, and configuration schema therefore match Graph compilation. The built-in
Registry currently contains `builtin.text_source`, `builtin.uppercase`,
`builtin.text_sink`, and the explicitly side-effecting development node
`builtin.stdout_text_sink` at version `1.0.0`. Studio runs that trusted Registry and
exposes authenticated local runtime metrics and Run/Stop control. General SDK
plugin discovery and remote production control remain follow-up work.
