# Muxiva Studio

Studio is the local visual environment bundled with the `muxiva` CLI. It edits
the same strict Graph v1 document used by validation and runtime compilation.

## Start Studio

```bash
muxiva studio
# Explicit cross-platform entry for the flagship voice project
./examples/voice-agent/run.sh --studio
```

Running `run.sh` without a mode also defaults to Studio in macOS/Windows shells.
Linux defaults to Headless Runtime, so use explicit `--studio` there.

With no argument, the CLI discovers the current project's `graph.json`, a
standalone `muxiva.graph.json`, or the flagship Voice Agent inside a Muxiva source
checkout, in that order. If none exists, it creates a new `muxiva.graph.json`
without overwriting any file. You can also pass a project directory or Graph:

```bash
muxiva studio my-agent
muxiva studio path/to/graph.json
```

Studio can open a Graph that does not yet validate so that you can repair it on
the canvas with exact diagnostics.

## Visual workflow

The Palette separates architecture layer from graph role. Filter by Transport,
Algorithm, Media, Control, or Utility, or search by capability, tag,
or Node type. Selecting a Node shows its summary, stable capability, detailed
Port schemas, implementation source, and Node-specific guide.

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
.muxiva/nodes/my_python_node/
├── muxiva.node.json
└── node.py
```

The package appears in the project Palette immediately. Python Nodes run through
the trusted local Host. A C++ dynamic library under
`.muxiva/native/<package_id>/` is loadable after Studio verifies its ABI v1
identity, version, role, and exact port shape against the Manifest. TypeScript
and Rust project source still requires an externally built supported artifact.

Selecting a project Node shows the exact source stored under `.muxiva/nodes/`
and offers **Edit in Node Lab**. Shared packages loaded through
Official Nodes loaded through the compatibility file `.muxiva/providers.json` show their exact
source as read-only, while project-owned
`.muxiva/nodes` remain editable. Selecting a compiled built-in shows its exact
Factory identity and a link to the authoritative Rust implementation.

## Runtime observability

The compact Runtime panel reports session totals. Open **◎ Observe** for the
dedicated live dashboard: per-Node throughput and process latency, per-Edge
rates and queue age, Node-owned SDK buffers, automatic hotspot verdicts, and
click-through details. See [Observability and bottleneck diagnosis](observability.md).
Run uses the current canvas snapshot; saving first is not required.

Studio no longer hosts Voice Room or other end-user pages. Deploy a Graph with
`muxiva serve` and host `examples/voice-agent/web/` independently. The microphone
page on a user's device therefore has no dependency on a developer's Studio.
See [Headless Runtime and standalone web client](headless-deployment.md).

## Security boundary

Studio listens on `127.0.0.1` by default and requires a random bearer token.
Saving or browsing project packages never executes source code. A trusted local
user must explicitly select **Run** before a language Host loads a package.

Studio is not a remote production control plane and must not be exposed to the
public internet.
