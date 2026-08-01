# Voxa Studio

Voxa Studio is a dependency-free local visual editor bundled into `voxa-cli`.
It edits the same strict Graph v1 JSON document used by `voxa validate` and the
runtime compiler—there is no separate browser-only graph format.

## Launch

```bash
cargo run -p voxa-cli -- studio examples/graphs/text-uppercase.v1.json
```

The CLI validates the graph, binds an ephemeral loopback port, prints a
tokenized URL, and opens the default browser. Use `--no-open` when running over
SSH or when you want to open the printed URL yourself:

```bash
cargo run -p voxa-cli -- studio graph.json --port 4173 --no-open
```

## Visual workflow

- Add trusted built-in source, transform, and sink Nodes from the palette.
- Drag Nodes to arrange the local canvas.
- Select a Node to edit its ID, type, and JSON configuration.
- Add or remove typed edges and configure capacity/overflow policy.
- Use Undo/Redo or edit the complete Graph v1 source in the JSON drawer.
- Validate at any time; diagnostics point back to the affected Node.
- Save writes formatted Graph v1 JSON atomically to the file passed to the CLI.

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

The editor exposes the trusted Node types supported by the current Graph v1
compiler: `builtin.text_source`, `builtin.uppercase`, and `builtin.text_sink`.
General plugin/SDK Node discovery and live runtime metrics require the planned
versioned Node Factory registry. Studio does not pretend those factories exist.
