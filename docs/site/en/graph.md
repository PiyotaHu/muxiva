# Graph and typed ports

Graph v1 is declarative configuration. It selects trusted Node Factories by an
exact identity and connects named ports with one exact Frame type.

```json
{
  "version": "voxa.graph/v1",
  "graph_id": "text-agent",
  "nodes": [
    {
      "id": "source",
      "node_type": "builtin.text_source",
      "language": "rust",
      "factory_version": "1.0.0",
      "node_config": {"text": "hello"}
    }
  ],
  "edges": []
}
```

## Factory identity

A Graph resolves each Node by:

```text
node_type + language + factory_version
```

Validation never guesses a version or silently selects a different language.

## Frame types

Ports accept exactly one of:

- `audio`
- `video`
- `text`
- `byte`
- `signal`
- `event`

There is no untyped `any` port. An Edge is valid only when source and target
port types match.

## Queue policy

Each Edge has a bounded capacity and overflow policy such as `block`,
`drop_oldest`, `drop_newest`, or `abort`. The appropriate policy depends on
whether freshness, completeness, or fail-fast behavior matters most.

## Security

Graph JSON cannot contain executable source, dynamic scripts, credentials, or
arbitrary remote resources. It references trusted Factories and configuration
only.
