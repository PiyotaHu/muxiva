import assert from 'node:assert/strict'
import test from 'node:test'
import { GraphNodeFactory, NodeRunner, TypeScriptTransformNode, defineTransformNode, runGraph } from '../index.js'

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

test('NodeRunner manages lifecycle and event callbacks', async () => {
  const implementation = defineTransformNode({
    onPrepare() { this.prefix = 'VOXA: ' },
    onProcess(frame) { return { text: this.prefix + frame.text.toUpperCase() } },
    onEvent(event) { if (!event.topic) throw new Error('missing topic') },
  })
  const runner = new NodeRunner(implementation)
  assert.deepEqual(await runner.process({ text: 'ready' }), { text: 'VOXA: READY' })
  await runner.event({ topic: 'agent.ready' })
  assert.equal(await runner.finish(), true)
  assert.equal(await runner.finish(), false)
  assert.equal(await runner.close(), true)
})

test('TypeScript factory executes inside registered Graph v1 runtime', async () => {
  const graph = JSON.stringify({
    version: 'voxa.graph/v1', graph_id: 'typescript-registered',
    nodes: [
      { id: 'source', node_type: 'builtin.text_source', language: 'rust', factory_version: '1.0.0', node_config: { text: 'hello' } },
      { id: 'upper', node_type: 'example.typescript.uppercase', language: 'typescript', factory_version: '1.0.0', node_config: {} },
      { id: 'sink', node_type: 'builtin.text_sink', language: 'rust', factory_version: '1.0.0', node_config: {} },
    ],
    edges: [
      { id: 'source-upper', from: { node_id: 'source', port: 'text_out' }, to: { node_id: 'upper', port: 'text_in' }, frame_type: 'text', queue_policy: { capacity: 8, overflow: 'block' } },
      { id: 'upper-sink', from: { node_id: 'upper', port: 'text_out' }, to: { node_id: 'sink', port: 'text_in' }, frame_type: 'text', queue_policy: { capacity: 8, overflow: 'block' } },
    ],
  })
  const factory = new GraphNodeFactory('example.typescript.uppercase', {
    onProcess(frame) { return { text: frame.text.toUpperCase() } },
  })
  const incompatible = new GraphNodeFactory('example.typescript.uppercase', {
    onProcess() { throw new Error('wrong exact Factory version selected') },
  }, { version: '2.0.0' })
  assert.equal(await runGraph(graph, [factory, incompatible]), 3)
})
