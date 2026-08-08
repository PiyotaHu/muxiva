import assert from 'node:assert/strict'
import test from 'node:test'
import { GraphNodeFactory, NodeRunner, TypeScriptTransformNode, defineTransformNode, runGraph } from '../index.js'

test('callbacks execute on a dedicated Worker and return synchronous output', async () => {
  const node = new TypeScriptTransformNode({ onProcess(frame) { return { text: frame.text.toUpperCase() } } })
  const result = await node.process({ text: 'muxiva' })
  assert.deepEqual(result, { text: 'MUXIVA' })
  await node.close()
})

test('JavaScript throws and Promise results are structured failures', async () => {
  const throwing = new TypeScriptTransformNode({ onProcess() { throw new Error('boom') } })
  await assert.rejects(throwing.process({}), /boom/)
  await throwing.close()

  const promising = new TypeScriptTransformNode({ onProcess() { return Promise.resolve('late') } })
  await assert.rejects(promising.process({}), (error) => error.code === 'MUXIVA_NODE_PROMISE_UNSUPPORTED')
  await promising.close()
})

test('admission is bounded and close discards late output', async () => {
  const node = new TypeScriptTransformNode({ onProcess(value) { return value } }, { capacity: 1 })
  const first = node.process({ sequence: 1 })
  await assert.rejects(node.process({ sequence: 2 }), (error) => error.code === 'MUXIVA_NODE_FULL')
  assert.deepEqual(await first, { sequence: 1 })
  assert.equal(await node.close(), true)
  await assert.rejects(node.process({}), (error) => error.code === 'MUXIVA_NODE_CLOSED')
})

test('NodeRunner manages lifecycle and event callbacks', async () => {
  const implementation = defineTransformNode({
    onPrepare() { this.prefix = 'MUXIVA: ' },
    onProcess(frame) { return { text: this.prefix + frame.text.toUpperCase() } },
    onEvent(event) { if (!event.topic) throw new Error('missing topic') },
  })
  const runner = new NodeRunner(implementation)
  assert.deepEqual(await runner.process({ text: 'ready' }), { text: 'MUXIVA: READY' })
  await runner.event({ topic: 'agent.ready' })
  assert.equal(await runner.finish(), true)
  assert.equal(await runner.finish(), false)
  assert.equal(await runner.close(), true)
})

test('TypeScript factory executes inside registered Graph v1 runtime', async () => {
  const graph = JSON.stringify({
    version: 'muxiva.graph/v1', graph_id: 'typescript-registered',
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
    onProcess(frame, ctx) {
      ctx.emit('text_out', { ...frame, text: frame.text.toUpperCase() })
      ctx.publishNotification('example.typescript.uppercased', { sequence: frame.sequence })
    },
  })
  const incompatible = new GraphNodeFactory('example.typescript.uppercase', {
    onProcess() { throw new Error('wrong exact Factory version selected') },
  }, { version: '2.0.0' })
  assert.equal(await runGraph(graph, [factory, incompatible]), 3)
})

test('schema-driven TypeScript source emits audio, video, bytes and multiple named outputs', async () => {
  const types = ['audio', 'video', 'byte', 'text']
  const source = new GraphNodeFactory('example.typescript.multimodal-source', {
    onProcess(_frame, context) {
      if (context.config.label !== 'demo') throw new Error('node_config was not delivered')
      return {
        audio_out: { kind: 'audio', sequence: 7, bytes: [0, 0], sampleRateHz: 8000, channels: 1, format: 'i16le', planar: false, samplesPerChannel: 1 },
        video_out: { kind: 'video', sequence: 7, pixelFormat: 'rgba8', bytes: [1, 2, 3, 4], width: 1, height: 1, stride: 4 },
        byte_out: { kind: 'byte', sequence: 7, bytes: [1, 2, 3], mediaType: 'application/octet-stream' },
        text_out: [{ kind: 'text', sequence: 7, text: 'one' }, { kind: 'text', sequence: 8, text: 'two' }],
      }
    },
  }, {
    kind: 'source',
    ports: types.map((frameType) => ({ name: `${frameType}_out`, direction: 'output', frameType })),
    configSchema: { type: 'object', properties: { label: { type: 'string' } } },
  })
  const sinks = types.map((frameType) => new GraphNodeFactory(`example.typescript.${frameType}-sink`, {
    onProcess(frame, context) {
      if (context.inputPort !== 'in' || frame.kind === undefined) throw new Error('typed sink input missing')
    },
  }, { kind: 'sink', ports: [{ name: 'in', direction: 'input', frameType }] }))
  const nodes = [{ id: 'source', node_type: 'example.typescript.multimodal-source', language: 'typescript', factory_version: '1.0.0', node_config: { label: 'demo' } }]
  const edges = []
  for (const frameType of types) {
    nodes.push({ id: `${frameType}-sink`, node_type: `example.typescript.${frameType}-sink`, language: 'typescript', factory_version: '1.0.0', node_config: {} })
    edges.push({ id: frameType, from: { node_id: 'source', port: `${frameType}_out` }, to: { node_id: `${frameType}-sink`, port: 'in' }, frame_type: frameType, queue_policy: { capacity: 8, overflow: 'block' } })
  }
  const graph = JSON.stringify({ version: 'muxiva.graph/v1', graph_id: 'typescript-multimodal', nodes, edges })
  assert.equal(await runGraph(graph, [source, ...sinks]), 5)
})
