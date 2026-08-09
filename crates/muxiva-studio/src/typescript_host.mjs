import { pathToFileURL } from 'node:url'
import { createInterface } from 'node:readline'

const [source, entrypoint, configJson] = process.argv.slice(1, 4)

// stdout is reserved for the framed Host protocol. Project diagnostics remain
// visible in Studio's runtime.log through the inherited stderr stream.
console.log = (...values) => console.error(...values)
console.info = (...values) => console.error(...values)

function write(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`)
}

function decodeFrame(value) {
  if (value === null || value === undefined) return undefined
  switch (value.kind) {
    case 'text':
    case 'event':
    case 'signal':
      return { ...value }
    case 'audio':
      return { ...value, data: Buffer.from(value.pcm_hex, 'hex') }
    case 'byte':
      return { ...value, data: Buffer.from(value.data_hex, 'hex') }
    default:
      throw new TypeError(`Studio TypeScript Host received unsupported Frame kind: ${String(value.kind)}`)
  }
}

function encodeFrame(value) {
  if (!value || typeof value !== 'object' || typeof value.kind !== 'string') {
    throw new TypeError('TypeScript Node emitted an invalid Frame')
  }
  if (value.kind === 'audio') {
    return {
      kind: 'audio',
      pcm_hex: Buffer.from(value.data ?? value.bytes ?? []).toString('hex'),
      sample_rate_hz: value.sample_rate_hz ?? value.sampleRateHz,
      channels: value.channels,
      sequence: value.sequence ?? 0,
    }
  }
  if (value.kind === 'byte') {
    return {
      kind: 'byte',
      data_hex: Buffer.from(value.data ?? value.bytes ?? []).toString('hex'),
      media_type: value.media_type ?? value.mediaType ?? 'application/octet-stream',
      sequence: value.sequence ?? 0,
    }
  }
  if (value.kind === 'text') {
    return { kind: 'text', text: String(value.text ?? ''), sequence: value.sequence ?? 0 }
  }
  if (value.kind === 'event') {
    return {
      kind: 'event', topic: value.topic, payload: value.payload ?? null,
      source: value.source ?? 'typescript.node', schema_version: value.schema_version ?? value.schemaVersion ?? 1,
      sequence: value.sequence ?? 0,
    }
  }
  throw new TypeError(`TypeScript Node emitted unsupported Frame kind: ${value.kind}`)
}

class NodeContext {
  constructor(nodeId, inputPort, config, streaming = false) {
    this.nodeId = nodeId
    this.inputPort = inputPort
    this.config = config
    this.streaming = streaming
    this.emissions = []
    this.signals = []
    this.events = []
    this.metrics = []
  }

  emit(port, frame) {
    const emission = { port, frame: encodeFrame(frame) }
    if (this.streaming) write({ kind: 'emission', ...emission })
    else this.emissions.push(emission)
  }

  emitSignal(name, payload = null) {
    const signal = { name, payload }
    if (this.streaming) write({ kind: 'signal', ...signal })
    else this.signals.push(signal)
  }

  publishNotification(topic, payload = null) {
    const event = { topic, payload }
    if (this.streaming) write({ kind: 'event', ...event })
    else this.events.push(event)
  }

  incrementCounter(name, delta = 1) {
    const metric = { name, operation: 'counter_add', value: Number(delta) }
    if (this.streaming) write({ kind: 'metric', ...metric })
    else this.metrics.push(metric)
  }

  setGauge(name, value) {
    const metric = { name, operation: 'gauge_set', value: Number(value) }
    if (this.streaming) write({ kind: 'metric', ...metric })
    else this.metrics.push(metric)
  }
}

async function loadNode() {
  const module = await import(`${pathToFileURL(source).href}?muxiva=${Date.now()}`)
  const symbol = entrypoint.includes(':') ? entrypoint.split(':', 2)[1] : entrypoint
  const exported = module[symbol]
  if (exported === undefined) throw new Error(`entrypoint export not found: ${symbol}`)
  if (typeof exported !== 'function') return exported
  try {
    return new exported(config)
  } catch (error) {
    if (!(error instanceof TypeError)) throw error
    return await exported(config)
  }
}

const config = JSON.parse(configJson)
const node = await loadNode()
if (!node || typeof node !== 'object') throw new TypeError('TypeScript entrypoint must export a Node object, class, or factory')

async function invoke(name, ...args) {
  const callback = node[name]
  if (typeof callback !== 'function') return undefined
  return await callback.apply(node, args)
}

write({ ready: true })
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity })
for await (const line of lines) {
  try {
    const command = JSON.parse(line)
    let response
    if (command.op === 'process') {
      const frame = decodeFrame(command.frame)
      const context = new NodeContext(command.node_id, command.input_port, config, true)
      const result = await invoke('onProcess', frame, context)
      if (result !== undefined && result !== null) {
        const values = result.kind ? { [command.default_output]: result } : result
        for (const [port, frames] of Object.entries(values)) {
          for (const item of Array.isArray(frames) ? frames : [frames]) context.emit(port, item)
        }
      }
      response = { ok: true, signals: context.signals, events: context.events, metrics: context.metrics }
    } else if (command.op === 'signal') {
      const context = new NodeContext(command.node_id, command.input_port, config)
      await invoke('onSignal', decodeFrame(command.signal), context)
      response = { ok: true, emissions: context.emissions, signals: context.signals, events: context.events, metrics: context.metrics }
    } else if (command.op === 'prepare') {
      await invoke('onPrepare', undefined, new NodeContext(command.node_id, undefined, config))
      response = { ok: true }
    } else if (command.op === 'finish') {
      await invoke('onFinish', undefined, new NodeContext(command.node_id, undefined, config))
      response = { ok: true }
    } else if (command.op === 'abort') {
      await invoke('onAbort', command.reason ?? 'aborted', new NodeContext(command.node_id, undefined, config))
      response = { ok: true }
    } else if (command.op === 'close') {
      write({ ok: true })
      break
    } else {
      throw new Error(`unknown Host operation: ${String(command.op)}`)
    }
    write(response)
  } catch (error) {
    write({ ok: false, error: `${error?.name ?? 'Error'}: ${error?.message ?? String(error)}` })
  }
}
