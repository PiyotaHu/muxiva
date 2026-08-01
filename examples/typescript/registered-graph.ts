import { GraphNodeFactory, runGraph } from '@voxa/core'

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

const factory = new GraphNodeFactory<{ text: string }, { text: string }>(
  'example.typescript.uppercase',
  { onProcess(frame) { return { text: frame.text.toUpperCase() } } },
)
const workers = await runGraph(graph, [factory])
console.log(`TypeScript Graph completed with ${workers} workers`)
