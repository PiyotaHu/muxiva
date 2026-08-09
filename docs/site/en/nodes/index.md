# Developer manual

Muxiva serves two extension paths. Application teams often already own an
Agent and need to connect it to real-time audio, interruption, multimodal data,
and observability. Infrastructure teams build ASR, TTS, RTC, and media Nodes.

Both enter the same typed Graph as project Node Packages, but their ownership
boundaries differ:

| Goal | Start here |
| --- | --- |
| Deploy an existing Agent into Muxiva | [Agent integration](agent-integration.md) |
| Try the file-capable reference Agent | [Pi coding Agent](pi-agent.md) |
| Build a Python algorithm or application Node | [Python](python.md) |
| Build a TypeScript Agent or application Node | [TypeScript](typescript.md) |
| Build a high-performance Rust Node | [Rust](rust.md) |
| Wrap an RTC, media, or vendor C++ SDK | [C++](cpp.md) |

## Project layout

```text
my-agent/
├── graph.json                         # Nodes, Ports, Edges, configuration
├── package.json                       # Agent dependencies and lock file
├── .env                               # local credentials, Git ignored
└── .muxiva/
    ├── nodes/my_agent/
    │   ├── muxiva.node.json           # Node contract
    │   └── node.ts                    # thin Agent → Muxiva adapter
    ├── agents/my-agent/               # pinned application-owned Agent repo
    └── workspaces/my-agent/            # file authority granted to the Agent
```

`graph.json` never contains executable source. A `muxiva.node/v1` Manifest
declares discovery and validation metadata. The Agent remains independently
versioned, tested, reviewed, and released by its application team.

## Node Manifest

| Field | Purpose |
| --- | --- |
| `package_id` | filesystem-safe project identity |
| `display_name` | Studio Palette label |
| `node_type` | stable Factory type |
| `language` | `rust`, `cpp`, `python`, or `typescript` |
| `factory_version` | exact Factory or adapter version |
| `kind` | `source`, `transform`, or `sink` |
| `category` | transport, algorithm, media, control, or utility |
| `capability` | stable searchable capability |
| `entrypoint` | language implementation entrypoint |
| `ports` | names, direction, Frame types, and semantic schemas |
| `config_schema` | JSON Schema for Node configuration |

## Studio-first workflow

Ordinary Nodes can be authored directly in Studio:

1. open Studio for the project Graph;
2. choose **Create Node**;
3. define the language, role, typed Ports, and configuration schema;
4. edit code and choose **Save & Register**;
5. drag the Node from the Palette and connect compatible Ports;
6. Validate, Run, and inspect calls, queues, messages, and media in Observe.

Do not paste an entire existing Agent into Studio. Keep the Agent in its own
repository and store only a stable adapter under `.muxiva/nodes/`. Follow the
complete [Agent integration SOP](agent-integration.md).
