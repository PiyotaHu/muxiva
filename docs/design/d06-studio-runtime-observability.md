# D06: Studio runtime observability and control

D06 turns Studio from a graph editor into a local run console without making
the browser a privileged runtime component.

## Control boundary

Studio owns at most one active `GraphRuntime`. `POST /api/v1/runtime/start`
compiles the submitted Graph v1 document against the trusted built-in Registry
and starts a fresh runtime. It returns `409` while another run is active.
`POST /api/v1/runtime/stop` calls the existing idempotent Runtime Stop handle.
The browser never receives a queue, Node, EventBus, or resource handle.

All runtime endpoints require the same bearer token as graph read/write APIs.
The run request is bounded by the Graph v1 one-megabyte document limit. A run
uses a snapshot of the current canvas, so unsaved editor changes can be tested
without changing the file on disk.

## Metrics

Core maintains one lock-free counter set per serialized Node execution domain:
prepare/process/signal/finish/abort totals, error and panic totals, aggregate
callback duration, and maximum callback duration. Existing Edge snapshots
provide capacity, queue length, high-water mark, enqueue/dequeue/drop/full
totals, blocked duration, oldest-frame age, and the bounded latest reason.

`GET /api/v1/runtime` returns one bounded presentation snapshot containing
runtime state, active Nodes, terminal outcome, Node counters, and Edge metrics.
Terminal snapshots remain available until the next run, which avoids losing
diagnostics when a short graph completes between browser polls.

## UI behavior

Studio exposes Run and Stop controls, a live summary panel, per-Edge pressure
meters, terminal failure details, active/error Node highlighting, and flow/drop
Edge coloring. Polling is deliberately low frequency and read-only; it never
blocks a media path or changes runtime scheduling.
