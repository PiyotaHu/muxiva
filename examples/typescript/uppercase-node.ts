import { NodeRunner, defineTransformNode } from '@voxa/core'

type TextValue = { kind: 'text'; text: string; sequence: number }

const uppercase = defineTransformNode<TextValue, TextValue>({
  onProcess(frame) {
    return { ...frame, text: frame.text.toUpperCase() }
  },
})

const runner = new NodeRunner(uppercase)
const output = await runner.process({ kind: 'text', text: 'hello voxa', sequence: 1 })
if (Array.isArray(output) || output == null || output.text !== 'HELLO VOXA') {
  throw new Error('unexpected Node output')
}
console.log(output.text)
await runner.finish()
await runner.close()
