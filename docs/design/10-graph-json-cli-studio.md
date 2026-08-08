# Stage 10 graph configuration and local tooling

Graph v1 is a bounded declarative document compiled through the same
`GraphBuilder` used by Rust callers.  It has an explicit `graph_id`, trusted
compiled-in node types, exact text ports in this first vertical slice, and
bounded queue policies.  The parser rejects unknown fields/types and documents
above 1 MiB.  JSON describes configuration only: no frames, credentials,
URLs, scripts, paths, or code are accepted.

`muxiva init`, `validate`, and `run` all use the same parser/compiler. `init`
creates a project directory by default, while a `.json` path retains the
single-file compatibility form. `validate`, `run`, and `studio` accept either a
project directory or Graph file. With no argument, `studio` discovers the
current workspace or creates a non-overwriting starter Graph, binds the exact
requested address, creates a local bearer token, and serves the Graph v1 schema,
graph, and validation endpoint. Studio intentionally opens invalid documents so
the visual diagnostics can repair them.
The token is carried in the initial URL fragment and removed from browser
history before authenticated API requests.

The bundled dependency-free Studio now provides an SVG graph canvas, trusted
Node palette, Node inspector, typed Edge editor, Undo/Redo, raw JSON editing,
live diagnostics, and atomic file saves. Static assets are served locally under
a strict Content Security Policy; no CDN, CORS, cookie, or remote code is used.
Canvas coordinates remain presentation-only and never pollute Graph v1.

General Python/TypeScript/C++ Node discovery, arbitrary registered factories,
and live runtime metrics remain blocked on the versioned graph/session factory
API. Studio exposes this boundary rather than inventing browser-only factories.
