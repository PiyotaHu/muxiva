# Python Nodes

Python is the fastest Studio project-Node path today. Text Source, Transform,
and Sink packages run through the trusted local Python development Host.

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

Declare matching ports:

```json
[
  {"name": "text_in", "direction": "input", "frame_type": "text"},
  {"name": "text_out", "direction": "output", "frame_type": "text"}
]
```

## Loading boundary

Saving, listing, or validating the package does not import Python source.
Studio starts a managed host and loads the declared entrypoint only after a
trusted local user selects **Run**.

## Current boundary

- text Frames are supported by the Studio project Host;
- Source, Transform, and Sink roles are supported;
- process isolation and multimodal project-package transport remain planned;
- the standalone Python SDK already exposes multimodal Frames and hosted Graph
  Factory APIs for programmatic development.
