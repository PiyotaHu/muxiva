import { GraphNodeFactory, runGraph, type GraphFrameType } from '@voxa/core'

const types: GraphFrameType[] = ['audio', 'video', 'byte', 'text']
const source = new GraphNodeFactory('example.typescript.multimodal-source', {
  onProcess(_frame, context) {
    return {
      audio_out: { kind: 'audio', sequence: 1, bytes: [0, 0], sampleRateHz: 8000, channels: 1, format: 'i16le', planar: false, samplesPerChannel: 1 },
      video_out: { kind: 'video', sequence: 2, pixelFormat: 'rgba8', bytes: [255, 0, 0, 255], width: 1, height: 1, stride: 4 },
      byte_out: { kind: 'byte', sequence: 3, bytes: [118, 111, 120, 97], mediaType: 'application/octet-stream' },
      text_out: { kind: 'text', sequence: 4, text: String(context.config.label) },
    }
  },
}, {
  kind: 'source',
  ports: types.map((frameType) => ({ name: `${frameType}_out`, direction: 'output', frameType })),
  configSchema: { type: 'object' },
})
const sinks = types.map((frameType) => new GraphNodeFactory(`example.typescript.${frameType}-sink`, {
  onProcess(frame, context) { console.log(context.inputPort, frame?.kind); return undefined },
}, { kind: 'sink', ports: [{ name: 'in', direction: 'input', frameType }] }))
const nodes: Record<string, unknown>[] = [{ id: 'source', node_type: 'example.typescript.multimodal-source', language: 'typescript', factory_version: '1.0.0', node_config: { label: 'hello' } }]
const edges: Record<string, unknown>[] = []
for (const frameType of types) {
  nodes.push({ id: `${frameType}-sink`, node_type: `example.typescript.${frameType}-sink`, language: 'typescript', factory_version: '1.0.0', node_config: {} })
  edges.push({ id: frameType, from: { node_id: 'source', port: `${frameType}_out` }, to: { node_id: `${frameType}-sink`, port: 'in' }, frame_type: frameType, queue_policy: { capacity: 8, overflow: 'block' } })
}
const graph = JSON.stringify({ version: 'voxa.graph/v1', graph_id: 'typescript-multimodal', nodes, edges })
console.log(`completed with ${await runGraph(graph, [source, ...sinks])} workers`)
