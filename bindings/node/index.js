import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { Worker } from 'node:worker_threads'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const platform = `${process.platform}-${process.arch}`
const candidates = [
  new URL(`./native/voxa.${platform}.node`, import.meta.url),
  new URL(`./native/voxa.${platform}-gnu.node`, import.meta.url),
  new URL(`./native/voxa.${platform}-musl.node`, import.meta.url),
  new URL('./native/voxa.node', import.meta.url),
]
const binary = candidates.find((url) => existsSync(fileURLToPath(url)))
if (!binary) throw new Error(`@voxa/core has no native binary for ${platform}; run npm run build`)
const native = require(fileURLToPath(binary))
export const { Runtime, Session, EventBus, Frame, AudioFrame, VideoFrame, TextFrame, ByteFrame, SignalFrame, EventFrame, NodeExecutionDomain } = native

const methodNames = ['onPrepare', 'onProcess', 'onSignal', 'onEvent', 'onFinish', 'onAbort']

export function defineTransformNode(implementation) {
  if (!implementation || typeof implementation !== 'object') throw new TypeError('implementation must be an object')
  if (typeof implementation.onProcess !== 'function') throw new TypeError('implementation.onProcess must be a function')
  return implementation
}

export class TypeScriptTransformNode {
  #worker; #capacity; #pending = new Map(); #closed = false; #next = 1
  constructor(implementation, { capacity = 16 } = {}) {
    if (!Number.isSafeInteger(capacity) || capacity < 1 || capacity > 65536) throw new RangeError('capacity must be between 1 and 65536')
    const methods = Object.fromEntries(methodNames.map((name) => [name, typeof implementation?.[name] === 'function' ? String(implementation[name]) : null]))
    this.#capacity = capacity
    this.#worker = new Worker(new URL('./worker.mjs', import.meta.url), { workerData: { addonPath: fileURLToPath(binary), capacity, methods } })
    this.#worker.on('message', (message) => {
      const pending = this.#pending.get(message.sequence)
      if (!pending) return
      this.#pending.delete(message.sequence)
      message.ok ? pending.resolve(message.value) : pending.reject(Object.assign(new Error(message.error.message), { code: message.error.code }))
    })
    this.#worker.on('error', (error) => this.#failAll(error))
    this.#worker.on('exit', (code) => { if (!this.#closed && code !== 0) this.#failAll(new Error(`Voxa Node worker exited with code ${code}`)) })
  }
  invoke(kind, payload = null) {
    if (this.#closed) return Promise.reject(Object.assign(new Error('node domain is closed'), { code: 'VOXA_NODE_CLOSED' }))
    if (this.#pending.size >= this.#capacity) return Promise.reject(Object.assign(new Error('node domain queue is full'), { code: 'VOXA_NODE_FULL' }))
    const sequence = this.#next++
    return new Promise((resolve, reject) => { this.#pending.set(sequence, { resolve, reject }); this.#worker.postMessage({ type: 'invoke', sequence, kind, payload }) })
  }
  prepare() { return this.invoke('prepare') }
  process(frame) { return this.invoke('process', frame) }
  signal(frame) { return this.invoke('signal', frame) }
  event(frame) { return this.invoke('event', frame) }
  finish() { return this.invoke('finish') }
  abort(reason) { return this.invoke('abort', reason) }
  async close() { if (this.#closed) return false; this.#closed = true; this.#failAll(Object.assign(new Error('node domain stopped'), { code: 'VOXA_NODE_STOPPED' })); this.#worker.postMessage({ type: 'close' }); await this.#worker.terminate(); return true }
  get outstanding() { return this.#pending.size }
  #failAll(error) { for (const pending of this.#pending.values()) pending.reject(error); this.#pending.clear() }
}

export class NodeRunner {
  #domain; #started = false; #finished = false
  constructor(implementation, options) { this.#domain = new TypeScriptTransformNode(defineTransformNode(implementation), options) }
  get domain() { return this.#domain }
  get outstanding() { return this.#domain.outstanding }
  async start() { if (!this.#started) { await this.#domain.prepare(); this.#started = true } return this }
  async process(frame) { await this.start(); if (this.#finished) throw new Error('NodeRunner is finished'); return this.#domain.process(frame) }
  async signal(frame) { await this.start(); return this.#domain.signal(frame) }
  async event(frame) { await this.start(); return this.#domain.event(frame) }
  async finish() { if (this.#finished) return false; await this.start(); await this.#domain.finish(); this.#finished = true; return true }
  abort(reason) { return this.#domain.abort(reason) }
  close() { return this.#domain.close() }
}

export class GraphNodeFactory {
  constructor(nodeType, implementation, {
    version = '1.0.0', inputPort = 'text_in', outputPort = 'text_out',
    kind = 'transform', ports, configSchema = {},
  } = {}) {
    if (typeof nodeType !== 'string' || nodeType.length === 0) throw new TypeError('nodeType must be a non-empty string')
    defineTransformNode(implementation)
    if (!['source', 'transform', 'sink'].includes(kind)) throw new TypeError('kind must be source, transform, or sink')
    if (ports !== undefined && !Array.isArray(ports)) throw new TypeError('ports must be an array')
    this.spec = {
      nodeType, version, inputPort, outputPort, kind,
      portsJson: ports === undefined ? undefined : JSON.stringify(ports),
      configSchemaJson: JSON.stringify(configSchema),
    }
    this.methods = Object.fromEntries(methodNames.map((name) => [name, typeof implementation[name] === 'function' ? String(implementation[name]) : null]))
  }
}

export function runGraph(graphJson, factories, { timeoutMs = 30_000 } = {}) {
  if (typeof graphJson !== 'string') return Promise.reject(new TypeError('graphJson must be a string'))
  if (!Array.isArray(factories) || factories.some((factory) => !(factory instanceof GraphNodeFactory))) {
    return Promise.reject(new TypeError('factories must contain GraphNodeFactory values'))
  }
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./graph-worker.mjs', import.meta.url), {
      workerData: {
        addonPath: fileURLToPath(binary), graphJson, timeoutMs,
        factories: factories.map(({ spec, methods }) => ({ spec, methods })),
      },
    })
    let settled = false
    worker.once('message', (message) => {
      settled = true
      void worker.terminate()
      if (message.ok) resolve(message.workerTotal)
      else reject(Object.assign(new Error(message.error.message), { code: message.error.code }))
    })
    worker.once('error', (error) => { if (!settled) { settled = true; reject(error) } })
    worker.once('exit', (code) => {
      if (!settled && code !== 0) reject(new Error(`Voxa Graph worker exited with code ${code}`))
    })
  })
}
