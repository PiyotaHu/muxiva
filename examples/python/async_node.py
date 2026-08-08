"""A Muxiva Python Node whose lifecycle uses async def."""

import asyncio

import muxiva


class AsyncPrefixNode(muxiva.TransformNode):
    async def on_prepare(self):
        await asyncio.sleep(0)
        self.prefix = "agent: "

    async def on_process(self, frame: muxiva.TextFrame):
        await asyncio.sleep(0.001)
        return muxiva.TextFrame(self.prefix + frame.text, sequence=frame.sequence)


with muxiva.NodeRunner(AsyncPrefixNode()) as runner:
    [output] = runner.process(muxiva.TextFrame("ready", sequence=1))
    assert output.text == "agent: ready"
    print(output.text)
