"""Run a Python Factory as part of a Registry-compiled Graph v1."""

import muxiva


class Uppercase:
    def on_process(self, frame: muxiva.TextFrame):
        return muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence)


GRAPH = r'''{
  "version":"muxiva.graph/v1",
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

factory = muxiva.GraphNodeFactory("example.python.uppercase", Uppercase)
workers = muxiva.run_graph(GRAPH, [factory])
print(f"Python Graph completed with {workers} workers")
