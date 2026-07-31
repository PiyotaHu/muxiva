import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { Worker } from 'node:worker_threads'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const platform = `${process.platform}-${process.arch}`
const candidates = [new URL(`./native/voxa.${platform}.node`, import.meta.url), new URL('./native/voxa.node', import.meta.url)]
const binary = candidates.find((url) => existsSync(fileURLToPath(url)))
if (!binary) throw new Error(`@voxa/core has no native binary for ${platform}; run npm run build`)
const native = require(fileURLToPath(binary))
export const { Runtime, Session, EventBus, Frame, AudioFrame, VideoFrame, TextFrame, ByteFrame, SignalFrame, EventFrame, NodeExecutionDomain } = native

const methodNames = ['onPrepare', 'onProcess', 'onSignal', 'onFinish', 'onAbort']
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
  finish() { return this.invoke('finish') }
  abort(reason) { return this.invoke('abort', reason) }
  async close() { if (this.#closed) return false; this.#closed = true; this.#failAll(Object.assign(new Error('node domain stopped'), { code: 'VOXA_NODE_STOPPED' })); this.#worker.postMessage({ type: 'close' }); await this.#worker.terminate(); return true }
  get outstanding() { return this.#pending.size }
  #failAll(error) { for (const pending of this.#pending.values()) pending.reject(error); this.#pending.clear() }
}

