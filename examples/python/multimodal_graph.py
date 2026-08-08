"""Schema-driven Python Source with four typed output ports and foreign Sinks."""

import json
import muxiva


class Source:
    def __init__(self, config):
        self.label = config["label"]

    def on_process(self):
        return {
            "audio_out": muxiva.AudioFrame(b"\0\0", 8000, 1, 1),
            "video_out": muxiva.VideoFrame(b"\xff\0\0\xff", 1, 1),
            "byte_out": muxiva.ByteFrame(b"muxiva", media_type="application/octet-stream"),
            "text_out": muxiva.TextFrame(self.label),
        }


class Sink:
    def on_process(self, frame, input_port):
        print(input_port, frame.frame_type)


TYPES = ("audio", "video", "byte", "text")
source_ports = [
    {"name": f"{kind}_out", "direction": "output", "frame_type": kind}
    for kind in TYPES
]
factories = [muxiva.GraphNodeFactory(
    "example.python.multimodal-source", Source, kind="source",
    ports_json=json.dumps(source_ports), pass_config=True,
    config_schema_json='{"type":"object"}',
)]
nodes = [{
    "id": "source", "node_type": "example.python.multimodal-source",
    "language": "python", "factory_version": "1.0.0",
    "node_config": {"label": "hello"},
}]
edges = []
for kind in TYPES:
    node_type = f"example.python.{kind}-sink"
    factories.append(muxiva.GraphNodeFactory(
        node_type, Sink, kind="sink",
        ports_json=json.dumps([{"name": "in", "direction": "input", "frame_type": kind}]),
    ))
    nodes.append({"id": f"{kind}-sink", "node_type": node_type, "language": "python", "factory_version": "1.0.0", "node_config": {}})
    edges.append({"id": kind, "from": {"node_id": "source", "port": f"{kind}_out"}, "to": {"node_id": f"{kind}-sink", "port": "in"}, "frame_type": kind, "queue_policy": {"capacity": 8, "overflow": "block"}})

graph = json.dumps({"version": "muxiva.graph/v1", "graph_id": "python-multimodal", "nodes": nodes, "edges": edges})
print(f"completed with {muxiva.run_graph(graph, factories)} workers")
