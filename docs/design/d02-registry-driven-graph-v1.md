# D02: Registry-driven Graph v1

Status: **Implemented**

D02 makes the executable Registry from D01 the only Node contract source for
Graph compilation and Studio discovery.

## Contract

- Every JSON Node explicitly supplies `node_type`, `language`, and
  `factory_version`.
- Factory lookup is exact; compilation never selects a latest version.
- Registry descriptors supply kind, ports, frame types, lifecycle metadata,
  and configuration schema.
- Factory validation runs during compilation without allocating a Node.
- Converted configuration and `NodeFactorySelection` are retained in the
  compiled `NodeDefinition`.
- Studio's Node catalog is serialized from the same Registry.
- Built-in registrations are executable and have no special compiler switch.

GraphBuilder remains usable for programmatic graphs with manually attached
instances. Such graphs may leave `NodeDefinition::factory()` unset. Graph v1
compilation always sets it.

## Configuration boundary

JSON null, booleans, signed integers, finite floats, strings, arrays, and
objects convert into Voxa `Value`. Unsigned integers beyond `i64` are rejected
instead of silently losing precision. Graph documents and built-in text input
remain bounded.

The built-in source requires exactly one `text` field. The uppercase and sink
Factories require empty configuration. Errors identify the Node and
`node_config` JSON Pointer before runtime startup.

## Implemented follow-up

D03 now materializes all selected Node instances from
`NodeDefinition::factory()`, attaches Edge policies, executes the graph through
the general concurrent Runtime, and reports its terminal outcome. D04 remains
responsible for registering Python, TypeScript, and C++ bridge factories into
this execution path.
