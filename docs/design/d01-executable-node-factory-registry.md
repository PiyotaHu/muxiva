# D01: Executable Node Factory Registry

Status: **Implemented Core contract**

The `voxa-core` registry is the trusted boundary between pure graph data and
runtime node instances. A registration is keyed by the exact tuple:

```text
(node_type, implementation_language, factory_version)
```

Versions are opaque exact-match protocol identifiers. The registry does not
silently choose a newest version because lexical ordering is not a safe
compatibility policy and reproducible graphs must not change behavior when a
new factory is installed.

Each `NodeRegistration` owns:

- type-level `NodeDescriptor` metadata;
- implementation language;
- exact factory contract version; and
- a thread-safe executable `NodeFactory`.

The type-level descriptor can be rebound to a graph-local `NodeId`; every port
owner is rebound at the same time. Registration validates descriptor
invariants before metadata becomes discoverable.

`NodeFactory::validate_config` is deterministic and side-effect free so a
compiler or Studio can validate configuration without allocating a Node.
`NodeFactory::create` returns a fresh `Box<dyn Node>` but invokes no lifecycle
hook. Only `GraphRunner` or `ConcurrentRuntime` owns prepare, process, finish,
and abort.

Both validation and creation run behind a panic boundary. Ordinary failures
retain a stable factory code and message; panic failures report the node type,
language, version, graph-local node ID, and failing stage without unwinding
through graph startup.

## Deliberate follow-ups

D01 provides the executable registry contract, not the complete JSON runtime:

- D02 now makes Graph v1 carry and validate exact Factory versions and uses
  the Registry as the only descriptor/config source.
- D03 now materializes every graph Node and Edge policy and starts the complete
  concurrent runtime from `voxa run`.
- D04 will register Python, TypeScript, and C++ bridge factories through this
  same contract.

There is no dynamic library loading, remote code fetch, implicit version
fallback, or lifecycle execution in the registry.
