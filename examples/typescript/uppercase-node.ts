import { NodeRunner, defineTransformNode } from '@muxiva/core'

type TextValue = { kind: 'text'; text: string; sequence: number }

const uppercase = defineTransformNode<TextValue, TextValue>({
  onProcess(frame) {
    return { ...frame, text: frame.text.toUpperCase() }
  },
})

const runner = new NodeRunner(uppercase)
const output = await runner.process({ kind: 'text', text: 'hello muxiva', sequence: 1 })
if (Array.isArray(output) || output == null || output.text !== 'HELLO MUXIVA') {
  throw new Error('unexpected Node output')
}
console.log(output.text)
await runner.finish()
await runner.close()
