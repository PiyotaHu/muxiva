import assert from 'node:assert/strict'
import test from 'node:test'
import { defineAgentNode, SentenceChunker } from '../index.js'

test('Agent Node streams semantic output and suppresses stale work after cancellation', async () => {
  let release
  const Node = defineAgentNode({
    createDriver() {
      return {
        async run(_prompt, sink) {
          sink.text('first')
          await new Promise((resolve) => { release = resolve })
          sink.text('stale')
        },
        cancel() {},
      }
    },
  })
  const node = new Node({ max_results_per_wakeup: 32 })
  const scheduled = []
  node.onProcess(
    { kind: 'text', text: 'hello', sequence: 7 },
    { inputPort: 'prompt_in', scheduleNextTick: (delay) => scheduled.push(delay) },
  )
  await new Promise((resolve) => setImmediate(resolve))
  node.onSignal(
    { name: 'muxiva.voice.speech.started', sequence: 8 },
    { scheduleNextTick: (delay) => scheduled.push(delay) },
  )
  release()
  await new Promise((resolve) => setImmediate(resolve))
  const output = []
  node.onProcess(
    undefined,
    { inputPort: undefined, emit: (port, frame) => output.push({ port, frame }) },
  )
  assert.deepEqual(scheduled, [20, 1])
  assert.equal(output.some(({ frame }) => frame.text === 'stale'), false)
  assert.equal(output.at(-1).frame.topic, 'muxiva.agent.response.cancelled')
})

test('SentenceChunker emits speech-sized chunks and flushes its remainder', () => {
  const chunker = new SentenceChunker({ maximumCharacters: 8 })
  assert.deepEqual(chunker.push('你好。继续'), ['你好。'])
  assert.deepEqual(chunker.flush(), ['继续'])
})
