import assert from 'node:assert/strict'
import test from 'node:test'
import { TypeScriptTransformNode } from '../index.js'

test('callbacks execute on a dedicated Worker and return synchronous output', async () => {
  const node = new TypeScriptTransformNode({ onProcess(frame) { return { text: frame.text.toUpperCase() } } })
  const result = await node.process({ text: 'voxa' })
  assert.deepEqual(result, { text: 'VOXA' })
  await node.close()
})

test('JavaScript throws and Promise results are structured failures', async () => {
  const throwing = new TypeScriptTransformNode({ onProcess() { throw new Error('boom') } })
  await assert.rejects(throwing.process({}), /boom/)
  await throwing.close()

  const promising = new TypeScriptTransformNode({ onProcess() { return Promise.resolve('late') } })
  await assert.rejects(promising.process({}), (error) => error.code === 'VOXA_NODE_PROMISE_UNSUPPORTED')
  await promising.close()
})

test('admission is bounded and close discards late output', async () => {
  const node = new TypeScriptTransformNode({ onProcess(value) { return value } }, { capacity: 1 })
  const first = node.process({ sequence: 1 })
  await assert.rejects(node.process({ sequence: 2 }), (error) => error.code === 'VOXA_NODE_FULL')
  assert.deepEqual(await first, { sequence: 1 })
  assert.equal(await node.close(), true)
  await assert.rejects(node.process({}), (error) => error.code === 'VOXA_NODE_CLOSED')
})

