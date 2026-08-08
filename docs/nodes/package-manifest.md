# `muxiva.node/v1` package Manifest

Each project package contains a `muxiva.node.json` file and one language source
file. A Python package created by Studio looks like this:

```text
.muxiva/nodes/my_asr/
├── muxiva.node.json
└── node.py
```

```json
{
  "format": "muxiva.node/v1",
  "package_id": "my_asr",
  "display_name": "My streaming ASR",
  "node_type": "example.streaming_asr",
  "language": "python",
  "factory_version": "1.0.0",
  "kind": "transform",
  "entrypoint": "node:MyNode",
  "category": "algorithm",
  "capability": "speech.asr.streaming",
  "summary": "Converts streaming speech into transcripts.",
  "ports": [
    { "name": "audio_in", "direction": "input", "frame_type": "audio", "schema": { "encoding": "pcm_s16le", "sample_rate_hz": 16000 } },
    { "name": "text_out", "direction": "output", "frame_type": "text", "schema": { "semantics": ["partial_transcript", "final_transcript"] } }
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

`category` is the architecture layer (`transport`, `algorithm`, `media`,
`control`, or `utility`). It is independent from `kind`, which only describes
the Node's source/transform/sink role in a Graph. Provider-owned Nodes reference
shared vendor metadata and credentials with `provider_id` and `connection_id`.
