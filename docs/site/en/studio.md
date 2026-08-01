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

The package appears in the project Palette immediately. Python Nodes run through
the trusted local Host. A C++ dynamic library under
`.voxa/native/<package_id>/` is loadable after Studio verifies its ABI v1
identity, version, role, and exact port shape against the Manifest. TypeScript
and Rust project source still requires an externally built supported artifact.

Selecting a project Node shows the exact source stored under `.voxa/nodes/`
and offers **Edit in Node Lab**. Selecting a compiled built-in shows its exact
Factory identity and a link to the authoritative Rust implementation.

## Runtime observability

The Runtime panel reports callback counts and duration, active and failed
Nodes, Edge throughput, queue occupancy, drops, and retained terminal outcome.
Run uses the current canvas snapshot; saving first is not required.

When a project provides `.voxa/web/index.html`, the toolbar exposes its project
experience, such as **Voice Room**. Studio saves the current valid graph before
opening it. The page is bearer-token protected, and only short-lived connection
fields explicitly marked `client_exposed: true` can be returned to it; server
API keys and bot credentials remain unavailable to browser code.

## Security boundary

Studio listens on `127.0.0.1` by default and requires a random bearer token.
Saving or browsing project packages never executes source code. A trusted local
user must explicitly select **Run** before a language Host loads a package.

Studio is not a remote production control plane and must not be exposed to the
public internet.
