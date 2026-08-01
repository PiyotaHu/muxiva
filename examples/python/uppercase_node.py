"""A complete synchronous Voxa Python Node."""

import voxa


class UppercaseNode(voxa.TransformNode):
    def on_process(self, frame: voxa.TextFrame):
        return voxa.TextFrame(
            frame.text.upper(),
            timestamp_ns=frame.timestamp_ns,
            sequence=frame.sequence,
        )


with voxa.NodeRunner(UppercaseNode()) as runner:
    [output] = runner.process(voxa.TextFrame("hello voxa", sequence=1))
    assert output.text == "HELLO VOXA"
    print(output.text)
