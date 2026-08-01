# Node packages

A project Node package keeps executable code outside Graph JSON. Its
`voxa.node/v1` Manifest declares discovery and validation metadata.

```text
.voxa/nodes/my_node/
├── voxa.node.json
└── node.py
```

## Manifest fields

| Field | Purpose |
| --- | --- |
| `package_id` | Filesystem-safe project identity |
| `display_name` | Human-readable Palette label |
| `node_type` | Stable Factory type name |
| `language` | `rust`, `cpp`, `python`, or `typescript` |
| `factory_version` | Exact Factory version |
| `kind` | `source`, `transform`, or `sink` |
| `entrypoint` | Language-specific implementation entrypoint |
| `ports` | Named direction and exact Frame type |
| `config_schema` | JSON Schema for Node configuration |

## Studio-first workflow

1. Open Studio for the project Graph.
2. Select **Create Node**.
3. Choose language and Node role.
4. Edit code, typed ports, and configuration schema.
5. Select **Save & Register**.
6. Add the package from the project Palette when its Host is available.
7. Connect compatible ports and run the Graph.

Saving does not execute code. A language Host must validate and activate a
package before it can enter a runnable Graph.

Choose a language guide:

- [Python](python.md)
- [TypeScript](typescript.md)
- [Rust](rust.md)
- [C++](cpp.md)
