# Stage 4 synchronous graph runtime

Date: 2026-08-01

Stage 4 implements the dependency-light, single-threaded graph boundary for
Muxiva's pre-release foundation. It builds a pure, deterministic DAG definition
and runs separately owned Node instances through the fixed lifecycle without
threads, async scheduling, queues, networking, FFI, Serde, or Python bindings.
This is an implementation report, not a quality or release-readiness claim.

## Delivered scope

- `GraphBuilder` validates stable Node and Edge IDs, explicit exact-type ports,
  configurations targeting declared Nodes, and DAG topology. Its stored node,
  Edge, and topological-order data is deterministic and contains no runtime
  callbacks or instances.
- `GraphRunner` accepts that pure definition plus a separate
  `BTreeMap<NodeId, Box<dyn Node>>`. It validates exact runtime attachments,
  prepares Nodes in topology order, calls each Source exactly once with `None`,
  delivers concrete Frames FIFO over explicit ports, and finishes in reverse
  topology order.
- Edge execution performs the exact type gate followed by optional named
  validation and transform policies. It supports forward, replace, drop,
  abort, and Stage 4 signal observation; callback panics and returned errors
  become the first deterministic abort reason.
- Abort cleanup applies to prepared Nodes at most once in reverse topology
  order. Panic diagnostics from an abort hook are retained without interrupting
  other cleanup.
- Synchronous per-Edge snapshots expose deliveries, drops, signals, and latest
  reasons. The queue-related fields deliberately remain neutral because this
  stage has no queue.
- The standalone `text_graph` example demonstrates
  `TextSource -> UppercaseTransform -> CollectSink` with explicit Text ports,
  a pure `GraphBuilder` definition, a separate instance map, and
  `GraphRunner`. It deterministically prints:

  ```text
  Collected uppercase text: HELLO, MUXIVA
  ```

  Its focused integration test invokes the public binary and asserts that
  exact output.

## Validation

Fresh validation completed from this Stage 4C worktree:

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| `cargo test --workspace --all-targets --all-features` | passed: 79 unit/integration tests |
| `cargo test --doc --workspace --all-features` | passed: 4 doc tests |
| all three `muxiva-examples` binaries | passed; `text_graph` produced the output above |
| `git diff --check` | passed |
| source/dependency audit | passed; production sources have no threads, Tokio, network, FFI, Serde, or Python surface |

The workspace total is 83 passing tests: 79 unit/integration tests plus 4 doc
tests. The dependency tree contains the pre-existing tracing and error-support
packages only; it introduces no Tokio, network, FFI, Serde, or Python package.

## Known debt and deferred boundaries

The following are intentional non-blocking limitations and must not be treated
as completed MVP functionality:

1. `ConfigSchema` is preserved as pure `Value` metadata but configuration
   values are not yet validated against it.
2. Named Edge policy attachment is exact per enabled `EdgeId`, but a registry
   or factory that resolves stable policy names into independently owned
   per-Edge instances is not implemented.
3. Stage 4 has no real bounded queues, queue-full/backpressure behavior,
   metric subscription service, or active external cancellation. Those are
   Stage 5 scheduler/runtime work.
4. Adjacent-node signal routing is deferred to Stage 6. Stage 4 only records a
   signal and invokes the originating policy's signal hook.
5. `Node::on_abort` is infallible. The runner can retain abort-hook panics as
   diagnostics, but cannot receive or preserve an abort-hook returned error.
6. Queue capacity, queue length, high-water mark, full count, blocked duration,
   and oldest-frame age metrics are neutral synchronous values; they are not
   evidence of queue behavior.
7. The runner is deliberately single-use and single-threaded. Streaming source
   polling, worker ownership, scheduling fairness, and multi-threaded metrics
   are outside this Stage 4 implementation.
8. No serializable graph DTO, node/policy registry, language binding, or
   cross-language runtime boundary is supplied by this stage.
9. **Resolved Stage 5 blocker:** `Node` and `EdgePolicy` now require `Send`,
   their trait-object maps preserve transferability, and compile-time tests
   cover both boxed callback types and both runtime maps. Synchronous fixtures
   use `Arc` with `Mutex` or atomics. This resolves worker ownership only; it
   adds no threads, async runtime, or concurrent scheduling to Stage 4.
10. `record_delivery` increments both enqueue and dequeue when an item is put
    on the in-memory work list, before the downstream dispatch is popped. The
    current synchronous metric therefore overcounts dequeue timing rather than
    modeling a real dequeue event.
11. `node_error` retains the error code and message but drops structured error
    context from a returned `MuxivaError`; that diagnostic fidelity must be
    preserved in a future error/abort contract refinement.
12. The original Stage 4 design status was stale after the runner landed; this
    Stage 4C update aligns its implementation status, but the contract still
    needs normal post-implementation review rather than being treated as a
    release-quality sign-off.
13. Fan-out behavior is tested, but coverage does not yet assert the required
    ascending `EdgeId` delivery order across multiple outgoing Edges. Add that
    focused ordering assertion before relying on fan-out order externally.

## Validation totals

The fresh run recorded **83 passing workspace tests**: **79 unit/integration
tests** and **4 doc tests**. This does not claim a quality-clean repository;
the deferred boundaries above remain open, while the callback-transferability
blocker is resolved.
