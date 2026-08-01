import { createRequire } from 'node:module'
import { parentPort, workerData, threadId } from 'node:worker_threads'

const native = createRequire(import.meta.url)(workerData.addonPath)

function revive(name, source) {
  if (!source) return () => undefined
  let normalized = source.trim()
  if (!normalized.startsWith('function') && !normalized.includes('=>')) normalized = `function ${normalized}`
  const fn = (0, eval)(`(${normalized})`)
  if (typeof fn !== 'function') throw new TypeError(`${name} must be a function`)
  return fn
}

const implementation = Object.fromEntries(Object.entries(workerData.methods).map(([name, source]) => [name, revive(name, source)]))
const dispatch = (command) => {
  const method = { prepare: 'onPrepare', process: 'onProcess', signal: 'onSignal', event: 'onEvent', finish: 'onFinish', abort: 'onAbort' }[command.kind]
  try {
    const payload = command.payloadJson === undefined ? undefined : JSON.parse(command.payloadJson)
    const value = implementation[method](payload)
    if (value && (typeof value === 'object' || typeof value === 'function') && typeof value.then === 'function') {
      throw Object.assign(new TypeError('Promise/thenable results are unsupported in Node V1'), { code: 'VOXA_NODE_PROMISE_UNSUPPORTED' })
    }
    domain.complete(command.sequence, JSON.stringify({ sequence: command.sequence, ok: true, value, threadId }))
  } catch (error) {
    const code = error.code ?? 'VOXA_NODE_EXCEPTION'
    const message = String(error.message ?? error)
    domain.fail(command.sequence, code, message, JSON.stringify({ sequence: command.sequence, ok: false, error: { code, message }, threadId }))
  }
  flush()
}
const domain = new native.NodeExecutionDomain(dispatch, workerData.capacity)

function flush() {
  for (const encoded of domain.drainCompletions()) parentPort.postMessage(JSON.parse(encoded))
}

parentPort.on('message', (message) => {
  if (message.type === 'close') {
    const closed = domain.close()
    parentPort.postMessage({ type: 'closed', closed })
    return
  }
  const outcome = domain.submit(message.sequence, message.kind, JSON.stringify(message.payload))
  if (outcome !== 'accepted') parentPort.postMessage({ sequence: message.sequence, ok: false, error: { code: `VOXA_NODE_${outcome.toUpperCase()}`, message: `node domain ${outcome}` }, threadId })
})
