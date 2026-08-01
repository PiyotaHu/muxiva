# D05: Schema-driven multimodal foreign Nodes

D05 removes the text-Transform restriction from the Python, TypeScript, and
C++ Graph hosts while preserving Graph v1 as a pure-data format.

## Contract

- Every Factory declares an exact Node kind: Source, Transform, or Sink.
- Every port has a stable name, direction, and one exact frame type: audio,
  video, text, or byte. There is no `Any` port.
- `config_schema` remains Registry metadata and `node_config` is copied into
  the trusted language constructor/callback context.
- A Source is called with no input. A Transform or Sink receives the exact
  input port identity. A lifecycle call returns zero or more named emissions;
  multiple frames may target the same port.
- Rust validates descriptor topology, frame shape, port type, queue bounds,
  deadlines, and shutdown. Foreign callbacks never gain direct Edge access.

## Language mappings

Python returns a dict from output port names to frame values or lists.
TypeScript uses a JSON wire union for owned audio/video/text/byte frames and an
object keyed by port. C++ uses an additive ABI with a borrowed array of
`voxa_named_frame_v1`; Rust copies every name and frame before the callback
storage may be reused.

The older single-text Factory APIs remain available. C++ keeps their exact ABI
layout and adds `voxa_runtime_run_multimodal_graph_v1` rather than resizing a
published v1 struct.

## Current media boundary

PCM audio, opaque bytes, text, and packed RGBA8 video are supported across all
three hosts. Python Core frames also support YUV420P, but the TypeScript and C++
graph wire contracts deliberately reject it until plane descriptors are added.
