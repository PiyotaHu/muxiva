# Graph v1 reference

Graph v1 is Voxa's strict, pure-data graph protocol. The root requires
`version` (`voxa.graph/v1`), `graph_id`, `nodes`, and `edges`. Unknown fields
are rejected and the complete document is limited to 1 MiB.

The machine-readable JSON Schema is bundled at
`crates/voxa-graph-json/schema/graph-v1.schema.json` and served by Studio at
`GET /api/v1/schema/graph-v1`.

## Node identity and Factory selection

Every Node requires:

```json
{
  "id": "source",
  "node_type": "builtin.text_source",
  "language": "rust",
  "factory_version": "1.0.0",
  "node_config": { "text": "hello" }
}
```

`node_type`, `language`, and `factory_version` form one exact Registry lookup.
There is no implicit newest version, fallback language, or default Factory
version. This makes a Graph reproducible when multiple implementations are
installed.

The compiler obtains the Node kind, ports, frame types, lifecycle metadata,
configuration schema, validator, and executable Factory from that one
registration. It does not maintain a separate Node switch. Configuration is
converted into Voxa's closed `Value` algebra, validated before Node creation,
and preserved in the compiled `NodeDefinition`.

Supported language spellings are `rust`, `cpp`, `python`, and `typescript`.
Only registrations actually installed in the compiler's Registry resolve.

## Built-in registrations

Voxa currently ships these exact Rust registrations at Factory version
`1.0.0`:

| Node type | Kind | Inputs | Outputs | Configuration |
| --- | --- | --- | --- | --- |
| `builtin.text_source` | Source | — | `text_out: text` | exactly one `text` string, at most 256 KiB |
| `builtin.uppercase` | Transform | `text_in: text` | `text_out: text` | empty object |
| `builtin.text_sink` | Sink | `text_in: text` | — | empty object |

Studio obtains the same catalog from `GET /api/v1/registry/nodes`; its Palette
and ports are not hard-coded separately.

## Edges

Every Edge names exact source/output and target/input ports, an exact
`frame_type`, and a bounded queue policy:

```json
{
  "id": "source-upper",
  "from": { "node_id": "source", "port": "text_out" },
  "to": { "node_id": "upper", "port": "text_in" },
  "frame_type": "text",
  "queue_policy": { "capacity": 32, "overflow": "block" }
}
```

`capacity` is non-zero. Supported overflow spellings are `block`,
`drop_oldest`, `drop_newest`, and `abort`. An Edge never performs an implicit
type conversion.

## Pre-alpha migration

Graphs created before D02 must add an explicit `factory_version` to every
Node. The current built-ins use `"factory_version": "1.0.0"`. Missing versions
are rejected instead of guessed.

Run `voxa validate` before `voxa run` or saving from Studio. Diagnostics carry
a stable code and JSON Pointer such as `/nodes/0/node_config`.

## Validation and execution

`voxa validate` parses and compiles against the Registry without allocating a
Node or invoking a lifecycle callback. `voxa run` performs the same compilation,
creates every exact Factory selection before startup, attaches the declared
bounded Edge policies, and starts the graph through `ConcurrentRuntime`.

The CLI currently installs the built-in Rust Registry. Embedders can call
`start_registered_runtime` with their own trusted registrations. Foreign-language
SDK hosts can register exact-version text Transform factories with
`voxa.GraphNodeFactory`/`voxa.run_graph`, `GraphNodeFactory`/`runGraph`, or
`voxa::GraphNodeFactory`/`Runtime::run_graph`. Graph files never load code or
fetch implementations by themselves.

Execution and shutdown waits are bounded with `--timeout-ms` (default 30000)
and `--shutdown-timeout-ms` (default 5000). A terminal abort reports its stable
error code, category, stage, Node, and message. A timeout reports active Nodes,
requests runtime stop, and performs only the configured bounded cleanup wait.
