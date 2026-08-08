# Stage 9: Python and Node execution domains

Status: pre-release implementation.

## Shared Core gate

`ForeignNodeDriver` is the only path between Core and a language runtime. It
owns bounded command and completion mailboxes, byte budgets, strict or
unordered release, per-call deadlines, cancellation, late-output rejection,
bounded shutdown diagnostics, and exactly-once abort ownership. Commands and
completions contain owned Muxiva values only; no interpreter object or borrowed
language memory enters Core.

Foreign threads may accept commands and post completions, but they cannot call
an Edge, Node, EventBus subscriber, or runtime worker inline. Core consumes the
completion mailbox and remains the routing authority.

## Python domain

The `muxiva` PyO3 module exposes owned immutable frame wrappers, minimal
Runtime/Session/EventBus resource owners, and `PythonNodeExecutionDomain`.
Each node domain owns a bounded driver, a named OS thread, a fresh asyncio loop
created and destroyed on that thread, and its Python implementation reference.
Normal `def` and `async def` lifecycle hooks are normalized on that loop;
there is no `asyncio.run`, global shared loop, or interpreter call from a Rust
realtime/RTC thread.

Stop seals Core admission first, injects cancellation for active sequences,
drops results that arrive after cancellation/deadline, and waits only to the
configured shutdown deadline. Python exceptions are copied into bounded
structured failures. In-process execution isolates queues and scheduling but
does not claim to isolate the process-global GIL.

`isolated_process` is rejected as unsupported. It will remain unavailable
until authenticated, versioned bounded IPC, copied/shared-memory envelope
validation, crash mapping, cleanup acknowledgements, and forced reap tests all
exist.

## Node domain

The `@muxiva/core` napi-rs package exposes the matching owned API and a bounded
Node execution domain. Rust producers enqueue owned commands using nonblocking
admission. A Node-API threadsafe function schedules lifecycle invocation on
the JavaScript event loop; the foreign callback cannot execute JavaScript
directly. JS references are released only after the TSFN is closed and the
domain drains.

V1 TypeScript hooks are synchronous. A throw maps to a structured foreign
failure; a Promise or thenable is explicitly rejected as unsupported and
causes the same exactly-once abort path. Promise support is not emulated by
blocking a Rust worker.

## Frames and control traffic

The six language frame types are immutable owners. Constructors validate
their stable tag and required scalar/layout bounds and copy bytes/text before
return. Language views return immutable values or copies, never a
`FrameBuffer` pointer. Signal and EventBus delivery use the same per-node
bounded inbox as process work; slow subscribers cannot block publishers.

Python audio entry is limited to low-frequency/merged audio. The Rust
RealtimeInputProfile remains responsible for merging high-frequency RTC audio
before any Python call. Video conversion/transcoding is not a V1 foreign-node
operation even though an owned VideoFrame utility wrapper is available.

## Destruction order

Graph/session stop seals the driver, cancels work, and drains the language
domain. Language node references are then destroyed on their owning execution
thread. The Python interpreter or Node runtime is always the last resource to
close. A shutdown timeout yields diagnostics and never just abandons a live
interpreter reference.
