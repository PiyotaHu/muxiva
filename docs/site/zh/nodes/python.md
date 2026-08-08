# Python Node

Python 是当前最快的 Studio 项目 Node 开发路径。文本 Source、Transform 与 Sink
Package 可以通过可信本地 Python 开发 Host 运行。

```python
import muxiva

class Uppercase:
    def on_process(self, frame, ctx):
        ctx.emit(
            "text_out",
            muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence),
        )
        ctx.publish_notification("example.text.uppercased", {"sequence": frame.sequence})

class ClientEvent:
    def on_process(self, frame, ctx):
        ctx.emit("event_out", muxiva.EventFrame(
            "example.client.message", '{"text":"ready"}',
            source="example.client_event", sequence=frame.sequence,
        ))
```

`ctx.emit(port, frame)` 可以在不结束回调的情况下发送数据；
`ctx.emit_signal(name, payload)` 用于相邻图控制；
`ctx.publish_notification(topic, payload)` 用于向 Runtime NotificationBus 发布低频进程内通知。
返回 Frame 或 Port 映射仍作为兼容写法保留，新 Node 应优先使用显式 Context 动作。

声明匹配的 Port：

```json
[
  {"name": "text_in", "direction": "input", "frame_type": "text"},
  {"name": "text_out", "direction": "output", "frame_type": "text"}
]
```

## 加载边界

保存、列出或校验 Package 都不会 Import Python 源码。只有可信本地用户点击
**Run** 后，Studio 才启动受管 Host 并加载 Manifest 声明的入口。

## 当前边界

- Studio 项目 Host 支持 Text、Audio 输入和 Signal 回调；
- 输出 Port 可声明 Text、Audio、Event 和 Signal；Node 通常通过
  `ctx.emit_signal(...)` 发出 Signal，通过 `ctx.emit(...)` 发出 Event Frame；
- 该开发 Host 尚未实现 Byte 与 Video 项目 Node Frame；
- 支持 Source、Transform 与 Sink；
- 进程隔离和多模态项目 Package 传输仍在规划中；
- 独立 Python SDK 已为代码开发提供多模态 Frame 与 Hosted Graph Factory API。
