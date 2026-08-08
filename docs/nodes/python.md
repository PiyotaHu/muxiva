# Python Node development

The fastest path is **Studio → Create Node → Python**. Studio creates
`.muxiva/nodes/<package_id>/node.py` and its Manifest. Text-only Python source,
transform, and sink Nodes can be added to the Graph and run immediately through
Studio's local development Host. Saving or browsing a package never imports its
code; the trusted local Host loads it only when you press **Run**.

The Studio Python Host passes a lifecycle context into every callback. Emit
data, graph-local control, and runtime-wide notifications explicitly without
ending `on_process`:

```python
import muxiva

class Uppercase:
    def on_process(self, frame, ctx):
        ctx.emit(
            "text_out",
            muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence),
        )
        ctx.emit_signal("example.text.ready", {"sequence": frame.sequence})
        ctx.publish_event("example.text.uppercased", {"sequence": frame.sequence})
```

`ctx.emit` may be called repeatedly for different ports or Frames. Return
values remain supported by the Studio Host only as compatibility sugar. The
standalone Wheel's general Graph callback bridge still uses returned mappings;
bringing this context-action protocol to that bridge is a tracked SDK boundary.

See the [Python SDK reference](../sdk/python.md) for lifecycle, async callback,
configuration, and Agora boundaries.

## 中文

推荐从 Studio 的 **Create Node → Python** 开始。代码与 Manifest 会进入当前
项目 Node Library。当前文本类型的 Python Source、Transform、Sink 可直接加入
Graph 并在本地开发 Host 中运行；保存和浏览不会导入代码，只有点击 **Run** 才会
加载。`on_process(frame, ctx)` 可通过 `ctx.emit`、`ctx.emit_signal` 和
`ctx.publish_event` 分别发送数据、图内控制信号和全局事件，不需要用 `return`
结束回调。Graph JSON 只引用 Factory 身份，不保存 Python 源码。
