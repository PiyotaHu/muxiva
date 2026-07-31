# Graph v1 reference

Required root fields are `version` (`voxa.graph/v1`), `graph_id`, `nodes`, and
`edges`.  Nodes name a compiled-in trusted type and language.  Edges name exact
source/output and target/input ports, a matching `frame_type`, and bounded
`queue_policy.capacity`/`overflow`.  The current built-in registry supports
`builtin.text_source`, `builtin.uppercase`, and `builtin.text_sink`, all in
Rust and all using text ports.  Use `voxa validate` for registry and graph
diagnostics before `voxa run`.
