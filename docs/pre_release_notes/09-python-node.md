# Stage 9 pre-release report

## Delivered

- Language-neutral `ForeignNodeDriver` with bounded admission/completion,
  strict and unordered release, deadlines, cancellation, late-result discard,
  and one abort owner.
- Buildable PyO3/maturin distribution named `voxa` with owned Runtime,
  Session, six immutable Frame variants, EventBus, and per-node Python
  thread/asyncio execution domains.
- Buildable napi-rs package named `@voxa/core` with the matching owned API,
  bounded TSFN scheduling onto the JS event loop, synchronous node lifecycle,
  throw conversion, and Promise/thenable rejection.
- Real language import/load, copy ownership, thread/loop identity, independent
  domain, overflow, async wait, exception, deadline/cancel, stop, and late
  output tests.

## Explicitly unavailable

- Python `isolated_process` is rejected. It is not advertised until real
  authenticated/versioned IPC, shared-memory validation, child-crash mapping,
  cleanup, termination, and reap behavior are implemented and tested.
- TypeScript Promise-returning lifecycle hooks are rejected in V1.
- Dynamic package download, hot reload, video transcoding, CPU-heavy Python,
  and raw per-10/20 ms Python audio callbacks are unsupported.

## Technical debt

- Cross-platform wheel/npm binary publication and expanded Python/Node version
  matrices follow after the local macOS vertical slice.
- The public Runtime/Session wrappers are lifecycle owners rather than a claim
  of a complete general graph-authoring API; Stage 10 owns that API/registry.
- Advanced GIL/non-yield metrics export, process pools, zero-copy shared memory,
  Promise support, generated reference docs, and leak dashboards remain later
  work. Safety-relevant disabled modes fail explicitly.
- Python V1 accepts the `unordered` release declaration but deliberately keeps
  `max_in_flight=1`; concurrent coroutine execution awaits a task registry with
  independent cancellation and shutdown accounting.
- The local default `python3` shim is an obsolete x86_64 Python 3.7. Rust gates
  select the supported arm64 Python 3.13 explicitly with `PYO3_PYTHON`.
