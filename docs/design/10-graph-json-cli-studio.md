# Stage 10 graph configuration and local tooling

Graph v1 is a bounded declarative document compiled through the same
`GraphBuilder` used by Rust callers.  It has an explicit `graph_id`, trusted
compiled-in node types, exact text ports in this first vertical slice, and
bounded queue policies.  The parser rejects unknown fields/types and documents
above 1 MiB.  JSON describes configuration only: no frames, credentials,
URLs, scripts, paths, or code are accepted.

`voxa init`, `validate`, and `run` all use the same parser/compiler.  `studio`
currently validates the document and verifies exact requested port binding; the
secure HTTP/token server and bundled canvas remain a tracked follow-up rather
than an unsafe placeholder.  Python/TypeScript/C++ SDK parity remains blocked
on their general graph/session APIs.
