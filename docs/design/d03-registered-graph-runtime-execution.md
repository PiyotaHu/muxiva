# D03: Registered Graph Runtime execution

Status: **Implemented**

D03 closes the boundary between a reproducibly compiled Graph v1 document and
the general concurrent Muxiva Runtime. `muxiva run` is now an execution command,
not a second spelling of validation.

## Startup contract

1. Parse and compile the document against one trusted `NodeRegistry`.
2. Resolve the exact `(node_type, language, factory_version)` already retained
   in each `NodeDefinition`.
3. Create every Node before any lifecycle callback runs.
4. Attach the compiled graph and its bounded Edge policies.
5. Start all workers through `ConcurrentRuntime`.

If a selection is absent or Factory creation fails, startup stops before the
Runtime can enter its lifecycle. This avoids partially prepared graphs and
makes Factory failures deterministic. `muxiva validate` uses only the first step
and remains allocation-free and side-effect free.

The reusable Core entry points are `materialize_registered_nodes` and
`start_registered_runtime`. They accept an explicit Registry, so embedders can
install trusted registrations without changing the Graph compiler or Runtime.

## Terminal and timeout behavior

The CLI waits for a terminal Runtime result with a finite deadline. Successful
completion reports graph, Node, Edge, and worker counts. Abort output preserves
the stable error code, category, lifecycle stage, Node identity, and message.

On timeout, the CLI reports the Runtime state and active Nodes, requests stop,
and waits only for a second bounded cleanup deadline. Both limits reject zero
and values above one hour. Defaults are 30 seconds for execution and 5 seconds
for cleanup.

## Verified scenarios

- an exact registered Factory is created and executed by a concurrent worker;
- missing selections and Factory creation failures are pre-start errors;
- Node errors propagate as the Runtime's terminal abort result;
- timeout diagnostics identify live Nodes and bounded stop completes;
- the graph produced by `muxiva init` executes as three workers from the CLI;
- invalid input produces the same compiler diagnostics in `validate` and `run`.

## Deliberate boundary

The CLI currently installs Muxiva's compiled-in Rust built-ins. D03 does not add
dynamic library loading, code embedded in Graph JSON, or remote implementation
fetching. D04 now adapts the existing Python, TypeScript, and C++ execution
domains into versioned Registry factories so hosted text Transform Nodes use
this same Graph v1 path.
