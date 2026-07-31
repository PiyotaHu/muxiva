# Stage 10 graph configuration and local tooling

Graph v1 is a bounded declarative document compiled through the same
`GraphBuilder` used by Rust callers.  It has an explicit `graph_id`, trusted
compiled-in node types, exact text ports in this first vertical slice, and
bounded queue policies.  The parser rejects unknown fields/types and documents
above 1 MiB.  JSON describes configuration only: no frames, credentials,
URLs, scripts, paths, or code are accepted.

`voxa init`, `validate`, and `run` all use the same parser/compiler.  `studio`
validates the document, binds the exact requested address, creates a local
bearer token, and serves the Graph v1 schema, graph, and validation endpoint.
The token is carried in the initial URL fragment and removed from browser
history before authenticated API requests. The current page is a minimal
local-only foundation; the bundled canvas remains a tracked follow-up.
Python/TypeScript/C++ SDK parity remains blocked on their general graph/session
APIs.
