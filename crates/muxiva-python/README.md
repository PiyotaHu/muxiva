# Muxiva Python SDK

Develop synchronous or asynchronous Muxiva Nodes in Python. The native runtime
executes each Node on a dedicated thread and asyncio event loop with bounded
mailboxes and deadlines.

Install a published wheel from PyPI:

```bash
pip install muxiva
```

Tagged Muxiva releases publish CPython 3.8–3.14 wheels for Linux x86-64/ARM64,
macOS universal2, and Windows x86-64. The same artifacts and their SHA-256
checksums are attached to the GitHub release.

```python
import muxiva

class Uppercase(muxiva.TransformNode):
    def on_process(self, frame: muxiva.TextFrame):
        return muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence)

with muxiva.NodeRunner(Uppercase()) as runner:
    [output] = runner.process(muxiva.TextFrame("hello"))
    print(output.text)
```

Callbacks may use `def` or `async def`: `on_prepare`, `on_process`,
`on_signal`, `on_event`, `on_finish`, and `on_abort`. V1 supports
`isolation="in_process"` and one in-flight callback per Python Node.

See the [Python SDK guide](../../docs/sdk/python.md) and
[`examples/python`](../../examples/python).

`GraphNodeFactory` plus `run_graph` registers an exact-version Python text
Transform into Graph v1. Rust owns the concurrent graph while each Python Node
keeps its dedicated execution thread and asyncio loop.
