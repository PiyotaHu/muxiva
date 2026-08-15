# Python SDK

## Install

Published releases use `pip install muxiva`. Every `v*` release tag must match
the versions in the workspace `Cargo.toml` and Python `pyproject.toml`; CI builds,
installs, and tests each supported wheel before PyPI trusted publishing. From
this repository:

```bash
python -m pip install maturin
python -m maturin build --manifest-path crates/muxiva-python/Cargo.toml --release
python -m pip install target/wheels/muxiva-*.whl
```

The package includes PEP 561 type information (`py.typed` and native stubs).

## Develop a Node

```python
import muxiva

class Uppercase(muxiva.TransformNode):
    def on_process(self, frame: muxiva.TextFrame):
        return muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence)

with muxiva.NodeRunner(Uppercase()) as runner:
    [output] = runner.process(muxiva.TextFrame("hello", sequence=1))
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
factory = muxiva.GraphNodeFactory("example.python.uppercase", Uppercase)
worker_total = muxiva.run_graph(graph_json, [factory])
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
`crates/muxiva-python/tests/test_muxiva.py`.

## Provider boundary

The Python framework package is vendor-neutral. Python business integrations,
such as the Qwen Voice Node Pack, live with the application and register through
the common Node Pack Manifest. RTC transport is implemented by the separate C++
Agora provider; there is no Agora wrapper in the Python wheel.
