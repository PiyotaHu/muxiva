# Python SDK

## Install

Published releases will use `pip install voxa`. From this repository:

```bash
python -m pip install maturin
python -m maturin build --manifest-path crates/voxa-python/Cargo.toml --release
python -m pip install target/wheels/voxa-*.whl
```

The package includes PEP 561 type information (`py.typed` and native stubs).

## Develop a Node

```python
import voxa

class Uppercase(voxa.TransformNode):
    def on_process(self, frame: voxa.TextFrame):
        return voxa.TextFrame(frame.text.upper(), sequence=frame.sequence)

with voxa.NodeRunner(Uppercase()) as runner:
    [output] = runner.process(voxa.TextFrame("hello", sequence=1))
```

`on_prepare`, `on_process`, `on_signal`, `on_event`, `on_finish`, and
`on_abort` may be implemented with `def` or `async def`. The native domain owns
a dedicated OS thread and asyncio loop. Configure bounds with `NodeOptions`.

V1 accepts only `max_in_flight=1` and `isolation="in_process"`. The context
manager calls prepare once, finish on a clean exit, and always closes the
domain. On exceptional exit it invokes `on_abort` with the exception message
before closing. See `examples/python/uppercase_node.py` and `async_node.py`.

## Register a Graph v1 Factory

```python
factory = voxa.GraphNodeFactory("example.python.uppercase", Uppercase)
worker_total = voxa.run_graph(graph_json, [factory])
```

The Graph node must select `language: "python"` and the exact Factory version.
Each materialized Node receives a fresh value from the constructor and runs on
its own Python execution thread and asyncio loop. The complete installable
example is `examples/python/registered_graph.py`.

D05 adds `kind`, `ports_json`, `config_schema_json`, and `pass_config`. Ports
accept `audio`, `video`, `text`, and `byte`. A Source implements
`on_process()` with no input; a Transform or Sink may implement
`on_process(frame, input_port)`. Return one frame for a single output or a dict
such as `{"audio_out": frame, "text_out": [frame1, frame2]}` for named,
multi-port emission. A Sink returns `None`. When `pass_config=True`, the
constructor receives the Graph `node_config` dict. Graph JSON still cannot
import Python code. The end-to-end contract is exercised in
`crates/voxa-python/tests/test_voxa.py`.
