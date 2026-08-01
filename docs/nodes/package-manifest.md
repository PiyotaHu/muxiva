# `voxa.node/v1` package Manifest

Each project package contains a `voxa.node.json` file and one language source
file. A Python package created by Studio looks like this:

```text
.voxa/nodes/my_asr/
├── voxa.node.json
└── node.py
```

```json
{
  "format": "voxa.node/v1",
  "package_id": "my_asr",
  "display_name": "My streaming ASR",
  "node_type": "example.streaming_asr",
  "language": "python",
  "factory_version": "1.0.0",
  "kind": "transform",
  "entrypoint": "node:MyNode",
  "ports": [
    { "name": "audio_in", "direction": "input", "frame_type": "audio" },
    { "name": "text_out", "direction": "output", "frame_type": "text" }
  ],
  "config_schema": {
    "type": "object",
    "properties": {},
    "additionalProperties": false
  }
}
```

`package_id` is a filesystem-safe project identity. Graph compilation uses the
exact tuple `node_type + language + factory_version`. Ports accept `audio`,
`video`, `text`, `byte`, `signal`, and `event`; there is no untyped `any` Port.

The Manifest is discovery metadata, not permission to execute code. A language
Host must successfully activate the package before Studio allows it into a
runnable Graph.
