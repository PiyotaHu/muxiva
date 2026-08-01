# Python Node development

The fastest path is **Studio → Create Node → Python**. Studio creates
`.voxa/nodes/<package_id>/node.py` and its Manifest. Text-only Python source,
transform, and sink Nodes can be added to the Graph and run immediately through
Studio's local development Host. Saving or browsing a package never imports its
code; the trusted local Host loads it only when you press **Run**.

For the current programmatic API, install a built Wheel and register the
Factory explicitly:

```python
import voxa

class Uppercase:
    def on_process(self, frame, input_port):
        return {"text_out": voxa.TextFrame(
            frame.text.upper(), sequence=frame.sequence
        )}

factory = voxa.GraphNodeFactory(
    "example.uppercase",
    Uppercase,
    kind="transform",
    ports_json='''[
      {"name":"text_in","direction":"input","frame_type":"text"},
      {"name":"text_out","direction":"output","frame_type":"text"}
    ]''',
)
workers = voxa.run_graph(graph_json, [factory])
```

See the [Python SDK reference](../sdk/python.md) for lifecycle, async callback,
configuration, and Agora boundaries.

## 中文

推荐从 Studio 的 **Create Node → Python** 开始。代码与 Manifest 会进入当前
项目 Node Library。当前文本类型的 Python Source、Transform、Sink 可直接加入
Graph 并在本地开发 Host 中运行；保存和浏览不会导入代码，只有点击 **Run** 才会
加载。Graph JSON 只引用 Factory 身份，不保存 Python 源码。
