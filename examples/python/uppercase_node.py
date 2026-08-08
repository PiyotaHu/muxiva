"""A complete synchronous Muxiva Python Node."""

import muxiva


class UppercaseNode(muxiva.TransformNode):
    def on_process(self, frame: muxiva.TextFrame):
        return muxiva.TextFrame(
            frame.text.upper(),
            timestamp_ns=frame.timestamp_ns,
            sequence=frame.sequence,
        )


with muxiva.NodeRunner(UppercaseNode()) as runner:
    [output] = runner.process(muxiva.TextFrame("hello muxiva", sequence=1))
    assert output.text == "HELLO MUXIVA"
    print(output.text)
