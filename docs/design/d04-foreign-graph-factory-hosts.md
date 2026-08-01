# D04: Foreign Graph Factory hosts

Status: **Implemented text Transform contract**

D04 connects Python, TypeScript, and C++ Node implementations to the exact
Registry selection and concurrent Graph execution path completed in D01–D03.
The Graph remains pure data: language code is supplied explicitly by a trusted
host and is never imported, evaluated, or fetched because a JSON field asks for
it.

## Shared Core boundary

`ForeignNodeProvider` creates fresh `ForeignNodeInstance` values behind
`ForeignNodeFactoryAdapter`. The adapter is the only bridge into `NodeFactory`
and maps owned, port-addressed emissions and adjacent Signals back through
`NodeContext`. Factory creation does not invoke lifecycle callbacks; the
Runtime remains the sole lifecycle owner.

The protocol is language-neutral and supports arbitrary port-addressed output
internally. The first public SDK surface deliberately exposes one text input
and one text output so every language can ship the same fully tested contract.

## Host implementations

- Python exposes `GraphNodeFactory` and `run_graph`. Every graph Node receives
  a fresh constructor result and a dedicated OS thread plus asyncio loop.
- TypeScript exposes `GraphNodeFactory` and `runGraph`. The wrapper owns a
  dedicated Worker; Rust graph execution runs off its event loop and uses a
  bounded ThreadsafeFunction to invoke synchronous lifecycle callbacks.
- C++ adds a versioned `voxa_node_factory_v1` ABI and
  `voxa::GraphNodeFactory`. A synchronous `Runtime::run_graph` call copies
  registration metadata and creates fresh vtables before startup.

All three combine their trusted registrations with the same Rust built-ins,
compile Graph v1 through `compile_with_registry`, materialize exact versions,
and start `ConcurrentRuntime`.

## Safety and lifecycle

- Python and TypeScript interpreter objects never enter Core.
- TypeScript Promise results remain explicitly unsupported rather than blocking
  a graph worker indefinitely.
- C++ exceptions remain inside `noexcept` C ABI trampolines.
- Runtime waits are finite and timeout requests stop. Native C++ callbacks are
  cooperative and must return; process isolation for untrusted native code is
  outside the V1 ABI.
- Graph validation cannot construct a language Node.
- Registration metadata and callback output are copied across language
  boundaries.

## Deliberate V1 limits

Public foreign registrations currently support Transform Nodes with empty
`node_config`, one text input, and one text output. General configuration
schemas, audio/video/byte ports, foreign Sources and Sinks, multiple named
outputs, package discovery, and long-lived interactive runtime handles remain
follow-up work. These limits are explicit instead of being hidden behind an API
that cannot honor them.
