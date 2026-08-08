import { createRequire } from 'node:module'
import { parentPort, workerData } from 'node:worker_threads'

const native = createRequire(import.meta.url)(workerData.addonPath)

function revive(name, source) {
  if (!source) return () => undefined
  let normalized = source.trim()
  if (!normalized.startsWith('function') && !normalized.includes('=>')) normalized = `function ${normalized}`
  const fn = (0, eval)(`(${normalized})`)
  if (typeof fn !== 'function') throw new TypeError(`${name} must be a function`)
  return fn
}

const factories = new Map(workerData.factories.map(({ spec, methods }) => [
  `${spec.nodeType}@${spec.version}`,
  Object.fromEntries(Object.entries(methods).map(([name, source]) => [name, revive(name, source)])),
]))

function dispatch(command) {
  const implementation = factories.get(command.factoryKey)
  if (!implementation) return JSON.stringify({ ok: false, error: { code: 'MUXIVA_NODE_GRAPH_FACTORY', message: `unknown TypeScript factory ${command.nodeType}` } })
  const method = { prepare: 'onPrepare', process: 'onProcess', signal: 'onSignal', finish: 'onFinish', abort: 'onAbort' }[command.kind]
  try {
    const payload = command.payloadJson === undefined ? undefined : JSON.parse(command.payloadJson)
    const context = {
      nodeId: command.nodeId,
      inputPort: command.inputPort,
      config: JSON.parse(command.configJson),
      actions: [],
      emit(port, frame) { this.actions.push({ kind: 'emit', port, frame }) },
      emitSignal(name, payload = null) { this.actions.push({ kind: 'signal', name, payload }) },
      publishNotification(topic, payload = null) { this.actions.push({ kind: 'event', topic, payload }) },
    }
    let value = implementation[method](payload, context)
    if (value && (typeof value === 'object' || typeof value === 'function') && typeof value.then === 'function') {
      throw Object.assign(new TypeError('Promise/thenable results are unsupported in Node V1'), { code: 'MUXIVA_NODE_PROMISE_UNSUPPORTED' })
    }
    if (command.kind === 'process' && value && typeof value === 'object' &&
        typeof value.text === 'string' && value.kind === undefined) {
      value = { kind: 'text', sequence: payload?.sequence ?? 0, text: value.text }
    }
    return JSON.stringify({ ok: true, value, actions: context.actions })
  } catch (error) {
    return JSON.stringify({ ok: false, error: { code: error.code ?? 'MUXIVA_NODE_EXCEPTION', message: String(error.message ?? error) } })
  }
}

try {
  const workerTotal = await native.runRegisteredGraph(
    workerData.graphJson,
    workerData.factories.map(({ spec }) => spec),
    dispatch,
    workerData.timeoutMs,
  )
  parentPort.postMessage({ ok: true, workerTotal })
} catch (error) {
  parentPort.postMessage({ ok: false, error: { code: error.code ?? 'MUXIVA_NODE_GRAPH', message: String(error.message ?? error) } })
}
