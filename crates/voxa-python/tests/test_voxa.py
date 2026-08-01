import asyncio
import threading
import time

import pytest

import voxa


def test_six_immutable_owned_frames():
    frames = [
        voxa.TextFrame("hello"),
        voxa.ByteFrame(b"abc", media_type="application/octet-stream"),
        voxa.AudioFrame(b"\0\0" * 160, 16_000, 1, 160),
        voxa.VideoFrame(bytes(16), 2, 2),
        voxa.SignalFrame("voxa.test.signal", "payload"),
        voxa.EventFrame("voxa.test.event", "payload"),
    ]
    assert [frame.frame_type for frame in frames] == ["text", "byte", "audio", "video", "signal", "event"]
    with pytest.raises(AttributeError):
        frames[0].text = "changed"


def test_async_lifecycle_uses_one_private_loop_and_thread():
    class Node:
        def __init__(self):
            self.observed = []

        async def on_prepare(self):
            self.observed.append((threading.get_ident(), id(asyncio.get_running_loop())))

        async def on_process(self, frame):
            await asyncio.sleep(0.01)
            self.observed.append((threading.get_ident(), id(asyncio.get_running_loop())))
            return frame

    node = Node()
    domain = voxa.PythonNodeExecutionDomain(node)
    domain.prepare()
    output = domain.process(voxa.TextFrame("hello"))
    assert output[0].text == "hello"
    domain.close()
    assert len(set(node.observed)) == 1
    assert node.observed[0][0] != threading.get_ident()


def test_exception_deadline_and_isolated_process_rejection():
    class Broken:
        def on_process(self, frame):
            raise RuntimeError("private failure")

    domain = voxa.PythonNodeExecutionDomain(Broken())
    with pytest.raises(voxa.VoxaError, match="VOXA-PY-EXCEPTION"):
        domain.process(voxa.TextFrame("x"))

    with pytest.raises(voxa.VoxaError, match="VOXA-PY-ISOLATION-UNSUPPORTED"):
        voxa.PythonNodeExecutionDomain(object(), isolation="isolated_process")


def test_event_bus_only_enqueues_into_the_domain():
    class Subscriber:
        def __init__(self):
            self.thread = None

        async def on_event(self, event):
            await asyncio.sleep(0)
            self.thread = threading.get_ident()

    node = Subscriber()
    domain = voxa.PythonNodeExecutionDomain(node)
    bus = voxa.EventBus()
    bus.subscribe("voxa.test.event", domain)
    assert bus.publish(voxa.EventFrame("voxa.test.event", "hello")) == (1, 1, 0)
    deadline = time.monotonic() + 1
    while node.thread is None and time.monotonic() < deadline:
        time.sleep(0.001)
    assert node.thread is not None
    assert node.thread != threading.get_ident()
    bus.close()
    domain.close()


def test_high_level_node_runner_manages_lifecycle():
    class Uppercase(voxa.TransformNode):
        def __init__(self):
            self.lifecycle = []

        def on_prepare(self):
            self.lifecycle.append("prepare")

        def on_process(self, frame):
            return voxa.TextFrame(frame.text.upper(), sequence=frame.sequence)

        def on_finish(self):
            self.lifecycle.append("finish")

        def on_abort(self, reason):
            self.lifecycle.append(("abort", reason))

    node = Uppercase()
    with voxa.NodeRunner(node) as runner:
        [output] = runner.process(voxa.TextFrame("hello", sequence=7))
        assert output.text == "HELLO"
        assert output.sequence == 7

    assert node.lifecycle == ["prepare", "finish"]
    assert runner.is_closed


def test_node_runner_aborts_on_exceptional_context_exit():
    class Observed(voxa.TransformNode):
        def __init__(self):
            self.reason = None

        def on_abort(self, reason):
            self.reason = reason

    node = Observed()
    with pytest.raises(RuntimeError, match="stop now"):
        with voxa.NodeRunner(node):
            raise RuntimeError("stop now")

    assert node.reason == "stop now"


def test_python_factory_executes_inside_registered_graph_v1_runtime():
    calls = []

    class Uppercase:
        def on_prepare(self):
            calls.append("prepare")

        def on_process(self, frame):
            calls.append(("process", frame.text))
            return voxa.TextFrame(frame.text.upper())

        def on_finish(self):
            calls.append("finish")

    graph = r'''{
      "version":"voxa.graph/v1",
      "graph_id":"python-registered",
      "nodes":[
        {"id":"source","node_type":"builtin.text_source","language":"rust","factory_version":"1.0.0","node_config":{"text":"hello"}},
        {"id":"upper","node_type":"example.python.uppercase","language":"python","factory_version":"1.0.0","node_config":{}},
        {"id":"sink","node_type":"builtin.text_sink","language":"rust","factory_version":"1.0.0","node_config":{}}
      ],
      "edges":[
        {"id":"source-upper","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"upper","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}},
        {"id":"upper-sink","from":{"node_id":"upper","port":"text_out"},"to":{"node_id":"sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}}
      ]
    }'''
    factory = voxa.GraphNodeFactory("example.python.uppercase", Uppercase)
    assert voxa.run_graph(graph, [factory]) == 3
    assert calls == ["prepare", ("process", "hello"), "finish"]


def test_schema_driven_multimodal_source_and_sinks():
    import json

    received = {}

    class Source:
        def __init__(self, config):
            assert config == {"label": "demo"}

        def on_process(self):
            return {
                "audio_out": voxa.AudioFrame(b"\x00\x00", 8000, 1, 1, sequence=7),
                "video_out": voxa.VideoFrame(b"\x01\x02\x03\x04", 1, 1, sequence=7),
                "byte_out": voxa.ByteFrame(b"raw", media_type="application/octet-stream", sequence=7),
                "text_out": [voxa.TextFrame("one", sequence=7), voxa.TextFrame("two", sequence=8)],
            }

    def sink(name):
        class Sink:
            def on_process(self, frame, input_port):
                assert input_port == "in"
                received.setdefault(name, []).append(frame.frame_type)
        return Sink

    source_ports = [
        {"name": "audio_out", "direction": "output", "frame_type": "audio"},
        {"name": "video_out", "direction": "output", "frame_type": "video"},
        {"name": "byte_out", "direction": "output", "frame_type": "byte"},
        {"name": "text_out", "direction": "output", "frame_type": "text"},
    ]
    factories = [voxa.GraphNodeFactory(
        "example.python.multimodal_source", Source, kind="source",
        ports_json=json.dumps(source_ports),
        config_schema_json=json.dumps({"type": "object", "properties": {"label": {"type": "string"}}}),
        pass_config=True,
    )]
    for frame_type in ("audio", "video", "byte", "text"):
        factories.append(voxa.GraphNodeFactory(
            f"example.python.{frame_type}_sink", sink(frame_type), kind="sink",
            ports_json=json.dumps([{"name": "in", "direction": "input", "frame_type": frame_type}]),
        ))
    nodes = [{"id": "source", "node_type": "example.python.multimodal_source", "language": "python", "factory_version": "1.0.0", "node_config": {"label": "demo"}}]
    edges = []
    for frame_type in ("audio", "video", "byte", "text"):
        nodes.append({"id": f"{frame_type}-sink", "node_type": f"example.python.{frame_type}_sink", "language": "python", "factory_version": "1.0.0", "node_config": {}})
        edges.append({"id": frame_type, "from": {"node_id": "source", "port": f"{frame_type}_out"}, "to": {"node_id": f"{frame_type}-sink", "port": "in"}, "frame_type": frame_type, "queue_policy": {"capacity": 8, "overflow": "block"}})
    graph = json.dumps({"version": "voxa.graph/v1", "graph_id": "python-multimodal", "nodes": nodes, "edges": edges})
    assert voxa.run_graph(graph, factories) == 5
    assert received == {"audio": ["audio"], "video": ["video"], "byte": ["byte"], "text": ["text", "text"]}
