# Python Node

Python 是当前最快的 Studio 项目 Node 开发路径。文本 Source、Transform 与 Sink
Package 可以通过可信本地 Python 开发 Host 运行。

```python
import voxa

class Uppercase:
    def on_process(self, frame, input_port):
        return {
            "text_out": voxa.TextFrame(
                frame.text.upper(), sequence=frame.sequence
            )
        }
```

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

- Studio 项目 Host 支持 text Frame；
- 支持 Source、Transform 与 Sink；
- 进程隔离和多模态项目 Package 传输仍在规划中；
- 独立 Python SDK 已为代码开发提供多模态 Frame 与 Hosted Graph Factory API。
