# Voxa Studio

Studio is the local visual environment bundled with the `voxa` CLI. It edits
the same strict Graph v1 document used by validation and runtime compilation.

## Visual workflow

1. Drag a built-in or project Node from the Palette onto the canvas.
2. Drag an output port to a compatible input port.
3. Select a Node to inspect its Factory metadata, configuration, and implementation source.
4. Validate, run, inspect live metrics, and stop the Runtime.
5. Save formatted Graph JSON atomically.

Studio derives an Edge's Frame type from its port schemas. Incompatible audio,
video, text, byte, signal, and event ports cannot be connected.

## Create a Node in Studio

Select **Create Node**, choose a language and role, edit the starter code,
declare ports and configuration schema, then select **Save & Register**.

```text
.voxa/nodes/my_python_node/
├── voxa.node.json
└── node.py
```

The package appears in the project Palette immediately. Text Python Source,
Transform, and Sink Nodes can run through the trusted local development Host.
TypeScript, Rust, and C++ project packages are authorable but remain disabled in
runnable Graphs until their corresponding Studio build Hosts are implemented.

Selecting a project Node shows the exact source stored under `.voxa/nodes/`
and offers **Edit in Node Lab**. Selecting a compiled built-in shows its exact
Factory identity and a link to the authoritative Rust implementation.

## Runtime observability

The Runtime panel reports callback counts and duration, active and failed
Nodes, Edge throughput, queue occupancy, drops, and retained terminal outcome.
Run uses the current canvas snapshot; saving first is not required.

## Security boundary

Studio listens on `127.0.0.1` by default and requires a random bearer token.
Saving or browsing project packages never executes source code. A trusted local
user must explicitly select **Run** before a language Host loads a package.

Studio is not a remote production control plane and must not be exposed to the
public internet.
