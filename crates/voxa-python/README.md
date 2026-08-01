# Voxa Python SDK

Develop synchronous or asynchronous Voxa Nodes in Python. The native runtime
executes each Node on a dedicated thread and asyncio event loop with bounded
mailboxes and deadlines.

Published releases will use:

```bash
pip install voxa
```

```python
import voxa

class Uppercase(voxa.TransformNode):
    def on_process(self, frame: voxa.TextFrame):
        return voxa.TextFrame(frame.text.upper(), sequence=frame.sequence)

with voxa.NodeRunner(Uppercase()) as runner:
    [output] = runner.process(voxa.TextFrame("hello"))
    print(output.text)
```

Callbacks may use `def` or `async def`: `on_prepare`, `on_process`,
`on_signal`, `on_event`, `on_finish`, and `on_abort`. V1 supports
`isolation="in_process"` and one in-flight callback per Python Node.

See the [Python SDK guide](../../docs/sdk/python.md) and
[`examples/python`](../../examples/python).
