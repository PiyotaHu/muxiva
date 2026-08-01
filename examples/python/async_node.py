"""A Voxa Python Node whose lifecycle uses async def."""

import asyncio

import voxa


class AsyncPrefixNode(voxa.TransformNode):
    async def on_prepare(self):
        await asyncio.sleep(0)
        self.prefix = "agent: "

    async def on_process(self, frame: voxa.TextFrame):
        await asyncio.sleep(0.001)
        return voxa.TextFrame(self.prefix + frame.text, sequence=frame.sequence)


with voxa.NodeRunner(AsyncPrefixNode()) as runner:
    [output] = runner.process(voxa.TextFrame("ready", sequence=1))
    assert output.text == "agent: ready"
    print(output.text)
