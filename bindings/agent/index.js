const DEFAULT_CANCEL_SIGNALS = [
  'muxiva.turn.cancelled',
  'muxiva.agent.cancel',
  'muxiva.voice.speech.started',
]
const DEFAULT_PREVIOUS_ONLY_CANCEL_SIGNALS = [
  'muxiva.turn.cancelled',
  'muxiva.voice.speech.started',
]

function positiveInteger(value, fallback, maximum) {
  return Number.isSafeInteger(value) && value > 0 ? Math.min(value, maximum) : fallback
}

function nonNegativeInteger(value, fallback, maximum) {
  return Number.isSafeInteger(value) && value >= 0 ? Math.min(value, maximum) : fallback
}

function eventFrame(topic, payload, sequence) {
  return {
    kind: 'event',
    topic,
    payload,
    source: 'muxiva.agent',
    schema_version: 1,
    sequence,
  }
}

function timeoutError(stage, timeoutMs) {
  const error = new Error(`Agent ${stage} timed out after ${timeoutMs} ms`)
  error.code = `MUXIVA_AGENT_${stage.toUpperCase()}_TIMEOUT`
  error.stage = stage
  return error
}

function errorString(error, key) {
  const value = error && typeof error === 'object' ? error[key] : undefined
  return typeof value === 'string' ? value.trim() : ''
}

function capabilityId(value, label = 'capability') {
  const id = String(value ?? '').trim()
  if (!/^[a-z][a-z0-9._:-]{0,127}$/.test(id)) {
    throw new TypeError(`${label} id must match [a-z][a-z0-9._:-]{0,127}`)
  }
  return id
}

function normalizeCapabilityCatalog(capabilities = []) {
  if (!Array.isArray(capabilities)) throw new TypeError('capabilities must be an array')
  const seen = new Set()
  return capabilities.map((capability) => {
    const id = capabilityId(capability?.id)
    if (seen.has(id)) throw new TypeError(`duplicate capability id: ${id}`)
    seen.add(id)
    return Object.freeze({
      id,
      kind: capabilityId(capability?.kind ?? 'agent', 'capability kind'),
      description: String(capability?.description ?? '').trim(),
    })
  })
}

function normalizeRouteProfile(profile, capabilityIds, label = 'route') {
  const id = capabilityId(profile?.id, label)
  const capabilities = [...new Set((profile?.capabilities ?? []).map((value) => capabilityId(value)))]
  const requiredCapabilities = [...new Set(
    (profile?.requiredCapabilities ?? []).map((value) => capabilityId(value)),
  )]
  for (const capability of capabilities) {
    if (!capabilityIds.has(capability)) {
      throw new TypeError(`${label} ${id} references unknown capability: ${capability}`)
    }
  }
  for (const capability of requiredCapabilities) {
    if (!capabilities.includes(capability)) {
      throw new TypeError(`${label} ${id} requires capability not granted by the route: ${capability}`)
    }
  }
  return Object.freeze({
    id,
    capabilities: Object.freeze(capabilities),
    requiredCapabilities: Object.freeze(requiredCapabilities),
  })
}

/**
 * Vendor-neutral, deterministic capability-policy utility. Muxiva validates
 * declarations and decisions; applications own the actual match functions.
 */
export class CapabilityRouter {
  constructor({ capabilities = [], routes = [], fallback = { id: 'default', capabilities: [] } } = {}) {
    this.catalog = Object.freeze(normalizeCapabilityCatalog(capabilities))
    const capabilityIds = new Set(this.catalog.map((capability) => capability.id))
    if (!Array.isArray(routes)) throw new TypeError('routes must be an array')
    this.routes = Object.freeze(routes.map((route) => {
      if (typeof route?.match !== 'function') throw new TypeError('route.match must be a function')
      return Object.freeze({
        ...normalizeRouteProfile(route, capabilityIds),
        match: route.match,
        reason: String(route.reason ?? '').trim(),
      })
    }))
    this.fallback = normalizeRouteProfile(fallback, capabilityIds, 'fallback route')
  }

  capabilities() {
    return this.catalog
  }

  route(prompt) {
    const normalizedPrompt = Object.freeze({
      text: String(prompt?.text ?? '').trim(),
      sequence: Number(prompt?.sequence ?? 0),
    })
    for (const route of this.routes) {
      const match = route.match(normalizedPrompt)
      if (!match) continue
      const metadata = match === true ? {} : match
      if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) {
        throw new TypeError(`route ${route.id} match result must be true, false, or metadata object`)
      }
      return Object.freeze({
        id: route.id,
        capabilities: route.capabilities,
        requiredCapabilities: route.requiredCapabilities,
        reason: route.reason,
        metadata: Object.freeze({ ...metadata }),
      })
    }
    return Object.freeze({
      id: this.fallback.id,
      capabilities: this.fallback.capabilities,
      requiredCapabilities: this.fallback.requiredCapabilities,
      reason: 'fallback',
      metadata: Object.freeze({}),
    })
  }
}

function validateDriverRoute(driver, prompt) {
  if (typeof driver.route !== 'function') return undefined
  const catalog = normalizeCapabilityCatalog(
    typeof driver.capabilities === 'function' ? driver.capabilities() : [],
  )
  const capabilityIds = new Set(catalog.map((capability) => capability.id))
  const decision = driver.route(prompt)
  if (decision && typeof decision.then === 'function') {
    throw new TypeError('AgentDriver.route must be synchronous')
  }
  const profile = normalizeRouteProfile(decision, capabilityIds, 'driver route')
  const metadata = decision?.metadata ?? {}
  if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) {
    throw new TypeError('driver route metadata must be an object')
  }
  return Object.freeze({
    id: profile.id,
    capabilities: profile.capabilities,
    requiredCapabilities: profile.requiredCapabilities,
    reason: String(decision?.reason ?? '').trim(),
    metadata: Object.freeze({ ...metadata }),
  })
}

/**
 * Wrap a vendor-specific Agent driver in Muxiva's stable Agent Node contract.
 * The driver owns model/session/tool behavior; this adapter owns graph lifecycle,
 * cancellation, bounded streaming output, and stale-generation suppression.
 */
export class AgentTurnController {
  constructor({ createDriver, config = {} } = {}) {
  if (typeof createDriver !== 'function') throw new TypeError('createDriver must be a function')
      this.config = config
      this.queue = []
      this.generation = 0
      this.activeController = undefined
      this.activeDriver = undefined
      this.activeSequence = undefined
      this.closed = false
      this.tail = Promise.resolve()
      this.createDriver = createDriver
      this.driverBusy = false
      this.driverEpoch = 0
      this.maxQueueSize = positiveInteger(config.max_queue_size, 2048, 65536)
      this.maxResultsPerWakeup = positiveInteger(
        config.max_results_per_wakeup ?? config.max_results_per_tick,
        64,
        1024,
      )
      this.cancelSignals = new Set(
        Array.isArray(config.cancel_signals) ? config.cancel_signals : DEFAULT_CANCEL_SIGNALS,
      )
      this.previousOnlyCancelSignals = new Set(
        Array.isArray(config.previous_only_cancel_signals)
          ? config.previous_only_cancel_signals
          : DEFAULT_PREVIOUS_ONLY_CANCEL_SIGNALS,
      )
      this.firstOutputTimeoutMs = positiveInteger(
        config.agent_first_output_timeout_ms ?? config.first_output_timeout_ms,
        10_000,
        300_000,
      )
      this.turnTimeoutMs = positiveInteger(
        config.agent_turn_timeout_ms ?? config.turn_timeout_ms,
        60_000,
        600_000,
      )
      this.timeoutMessage = typeof config.timeout_message === 'string'
        ? config.timeout_message.trim()
        : ''
      this.failureMessage = typeof config.failure_message === 'string'
        ? config.failure_message.trim()
        : ''
      this.progressMessage = typeof config.progress_message === 'string'
        ? config.progress_message.trim()
        : ''
      this.progressDelayMs = nonNegativeInteger(config.progress_delay_ms, 0, 30_000)
      this.driver = this.makeDriver(undefined)
  }

    onProcess(frame, context) {
      if (context.inputPort === 'prompt_in') {
        if (frame?.kind !== 'text') throw new TypeError('prompt_in requires a TextFrame')
        this.start(frame.text, frame.sequence ?? 0)
        context.scheduleNextTick?.(20)
        return
      }
      if (context.inputPort === 'tick_in' || (context.inputPort == null && frame == null)) {
        this.drain(context)
        if (this.activeController || this.queue.length > 0) context.scheduleNextTick?.(20)
        return
      }
      throw new Error(`Agent Node received unsupported input Port: ${String(context.inputPort)}`)
    }

    onSignal(signal, context) {
      if (this.cancelSignals.has(signal?.name)) {
        const sequence = Number(signal?.sequence ?? 0)
        if (
          this.previousOnlyCancelSignals.has(signal?.name)
          && sequence > 0
          && this.activeSequence !== undefined
          && sequence <= this.activeSequence
        ) {
          console.error(`[MUXIVA][AGENT][cancel.ignored] signal=${signal.name} signal_sequence=${sequence} active_sequence=${this.activeSequence}`)
          return
        }
        this.cancel(signal?.name ?? 'cancelled', sequence)
        if (this.queue.length > 0) context?.scheduleNextTick?.(1)
      }
    }

    async onFinish() {
      this.closed = true
      this.cancel('runtime.finished', 0)
      await this.tail.catch(() => undefined)
      await Promise.race([
        Promise.resolve(this.driver.close?.()).catch(() => undefined),
        new Promise((resolve) => setTimeout(resolve, 2_000)),
      ])
    }

    onAbort(reason) {
      this.closed = true
      this.cancel(String(reason ?? 'runtime.aborted'), 0)
    }

    start(text, sequence) {
      const prompt = String(text).trim()
      if (!prompt || this.closed) return
      this.cancel('superseded', sequence)
      const generation = ++this.generation
      const controller = new AbortController()
      this.activeController = controller
      this.activeSequence = sequence
      this.tail = this.tail
        .catch(() => undefined)
        .then(() => this.executeTurn({ prompt, sequence, generation, controller }))
    }

    cancel(reason, sequence) {
      const controller = this.activeController
      if (!controller || controller.signal.aborted) return
      controller.abort(reason)
      this.activeDriver?.cancel?.(reason)
      this.activeController = undefined
      this.activeSequence = undefined
      this.generation += 1
      this.queue.length = 0
      this.queue.push({
        generation: this.generation,
        port: 'event_out',
        frame: eventFrame('muxiva.agent.response.cancelled', { reason }, sequence),
      })
    }

    async executeTurn({ prompt, sequence, generation, controller }) {
      if (generation !== this.generation || controller.signal.aborted || this.closed) return
      if (this.driverBusy) this.rotateDriver('previous_turn_unresponsive')
      const driver = this.driver
      const epoch = this.driverEpoch
      this.driverBusy = true
      this.activeDriver = driver
      this.enqueue(generation, 'event_out', eventFrame('muxiva.agent.response.started', {}, sequence))
      const startedAt = Date.now()
      let accepting = true
      let responseText = ''
      let firstTextEmitted = false
      let firstOutputTimer
      let turnTimer
      let progressTimer
      let rejectWatchdog
      const watchdog = new Promise((_, reject) => { rejectWatchdog = reject })
      const failForTimeout = (stage, timeoutMs) => {
        if (!accepting || controller.signal.aborted) return
        rejectWatchdog?.(timeoutError(stage, timeoutMs))
        controller.abort(stage)
        driver.cancel?.(stage)
      }
      const clearFirstOutputTimer = () => {
        if (!firstOutputTimer) return
        clearTimeout(firstOutputTimer)
        firstOutputTimer = undefined
        console.error(`[MUXIVA][AGENT][first_output] sequence=${sequence} duration_ms=${Date.now() - startedAt}`)
      }
      firstOutputTimer = setTimeout(
        () => failForTimeout('first_output', this.firstOutputTimeoutMs),
        this.firstOutputTimeoutMs,
      )
      turnTimer = setTimeout(
        () => failForTimeout('turn', this.turnTimeoutMs),
        this.turnTimeoutMs,
      )
      const sink = {
        text: (delta) => {
          if (!accepting || generation !== this.generation) return
          const value = String(delta)
          if (!value) return
          if (progressTimer) {
            clearTimeout(progressTimer)
            progressTimer = undefined
          }
          clearFirstOutputTimer()
          if (!firstTextEmitted) {
            firstTextEmitted = true
            console.error(`[MUXIVA][AGENT][first_text] sequence=${sequence} duration_ms=${Date.now() - startedAt}`)
          }
          responseText += value
          this.enqueue(generation, 'text_out', { kind: 'text', text: value, sequence })
        },
        event: (type, payload = {}) => {
          if (!accepting || generation !== this.generation) return
          if (type === 'tool.started') {
            clearFirstOutputTimer()
            console.error(`[MUXIVA][AGENT][first_activity] sequence=${sequence} type=tool.started duration_ms=${Date.now() - startedAt}`)
            if (
              !firstTextEmitted
              && this.progressMessage
              && this.progressDelayMs > 0
              && !progressTimer
            ) {
              progressTimer = setTimeout(() => {
                progressTimer = undefined
                if (!accepting || firstTextEmitted || generation !== this.generation) return
                firstTextEmitted = true
                responseText += this.progressMessage
                this.enqueue(generation, 'text_out', {
                  kind: 'text', text: this.progressMessage, sequence,
                })
                console.error(`[MUXIVA][AGENT][first_text] sequence=${sequence} source=progress duration_ms=${Date.now() - startedAt}`)
              }, this.progressDelayMs)
            }
          }
          this.enqueue(generation, 'event_out', eventFrame(`muxiva.agent.${type}`, payload, sequence))
        },
      }
      let removeAbortListener = () => undefined
      const aborted = new Promise((_, reject) => {
        const onAbort = () => reject(controller.signal.reason ?? new Error('Agent turn cancelled'))
        controller.signal.addEventListener('abort', onAbort, { once: true })
        removeAbortListener = () => controller.signal.removeEventListener('abort', onAbort)
      })
      let routedPrompt = { text: prompt, sequence }
      let routeError
      try {
        const route = validateDriverRoute(driver, routedPrompt)
        if (route) {
          routedPrompt = { ...routedPrompt, route }
          sink.event('route.selected', {
            route_id: route.id,
            capabilities: route.capabilities,
            required_capabilities: route.requiredCapabilities,
            reason: route.reason,
            metadata: route.metadata,
          })
          console.error(`[MUXIVA][AGENT][route.selected] sequence=${sequence} route=${route.id} capabilities=${route.capabilities.join(',') || 'none'} required=${route.requiredCapabilities.join(',') || 'none'}`)
        }
      } catch (error) {
        routeError = error
      }
      const driverRun = routeError
        ? Promise.reject(routeError)
        : Promise.resolve().then(() => driver.run(routedPrompt, sink, controller.signal))
      driverRun
        .finally(() => {
          if (this.driver === driver && this.driverEpoch === epoch) this.driverBusy = false
        })
        .catch(() => undefined)
      console.error(`[MUXIVA][AGENT][turn.started] sequence=${sequence} generation=${generation} epoch=${epoch}`)
      try {
        await Promise.race([driverRun, aborted, watchdog])
        if (!controller.signal.aborted && generation === this.generation) {
          this.enqueue(
            generation,
            'event_out',
            eventFrame('muxiva.agent.response.completed', { text: responseText }, sequence),
          )
          console.error(`[MUXIVA][AGENT][turn.completed] sequence=${sequence} duration_ms=${Date.now() - startedAt}`)
        }
      } catch (error) {
        const timedOut = error?.code === 'MUXIVA_AGENT_FIRST_OUTPUT_TIMEOUT'
          || error?.code === 'MUXIVA_AGENT_TURN_TIMEOUT'
        if (timedOut && generation === this.generation) {
          accepting = false
          this.rotateDriver(`timeout.${error.stage}`)
          this.enqueue(
            generation,
            'event_out',
            eventFrame('muxiva.agent.response.failed', {
              message: error.message,
              reason: `timeout.${error.stage}`,
              duration_ms: Date.now() - startedAt,
            }, sequence),
          )
          if (this.timeoutMessage) {
            this.enqueue(generation, 'text_out', { kind: 'text', text: this.timeoutMessage, sequence })
          }
          console.error(`[MUXIVA][AGENT][turn.timeout] sequence=${sequence} stage=${error.stage} duration_ms=${Date.now() - startedAt}`)
        } else if (!controller.signal.aborted && generation === this.generation) {
          accepting = false
          const recoverDriver = !(error && typeof error === 'object' && error.recoverDriver === false)
          const reason = errorString(error, 'reason') || errorString(error, 'code') || 'run.failed'
          const userMessage = errorString(error, 'userMessage') || this.failureMessage
          if (recoverDriver) this.rotateDriver(reason)
          console.error('[MUXIVA][AGENT][response.failed]', error?.stack ?? error?.message ?? String(error))
          this.enqueue(
            generation,
            'event_out',
            eventFrame('muxiva.agent.response.failed', {
              message: error?.message ?? String(error),
              reason,
              driver_recovered: recoverDriver,
            }, sequence),
          )
          if (userMessage) {
            this.enqueue(generation, 'text_out', { kind: 'text', text: userMessage, sequence })
          }
        }
      } finally {
        accepting = false
        removeAbortListener()
        if (firstOutputTimer) clearTimeout(firstOutputTimer)
        if (turnTimer) clearTimeout(turnTimer)
        if (progressTimer) clearTimeout(progressTimer)
        if (generation === this.generation) {
          this.activeController = undefined
          this.activeDriver = undefined
          this.activeSequence = undefined
        }
      }
    }

    makeDriver(state) {
      const driver = this.createDriver({ config: this.config, state })
      if (!driver || typeof driver.run !== 'function') {
        throw new TypeError('createDriver must return an object with run(prompt, sink, signal)')
      }
      return driver
    }

    rotateDriver(reason) {
      const previous = this.driver
      let state
      try {
        state = previous.snapshot?.()
      } catch (error) {
        console.error('[MUXIVA][AGENT][snapshot.failed]', error?.message ?? String(error))
      }
      previous.cancel?.(reason)
      this.driverEpoch += 1
      this.driver = this.makeDriver(state)
      this.driverBusy = false
      Promise.resolve(previous.close?.()).catch((error) => {
        console.error('[MUXIVA][AGENT][retired_driver.close_failed]', error?.message ?? String(error))
      })
      console.error(`[MUXIVA][AGENT][driver.rotated] epoch=${this.driverEpoch} reason=${reason}`)
    }

    enqueue(generation, port, frame) {
      if (generation !== this.generation || this.closed) return
      if (this.queue.length >= this.maxQueueSize) {
        this.activeController?.abort('output.queue_full')
        this.activeDriver?.cancel?.('output.queue_full')
        this.queue.length = 0
        this.queue.push({
          generation,
          port: 'event_out',
          frame: eventFrame(
            'muxiva.agent.response.failed',
            { message: `Agent output queue exceeded ${this.maxQueueSize} items` },
            frame.sequence ?? 0,
          ),
        })
        return
      }
      this.queue.push({ generation, port, frame })
    }

    drain(context) {
      for (let index = 0; index < this.maxResultsPerWakeup; index += 1) {
        const item = this.queue.shift()
        if (!item) return
        if (item.generation !== this.generation) continue
        context.emit(item.port, item.frame)
        if (item.port === 'text_out') {
          context.publishNotification?.('muxiva.agent.response.delta', { text: item.frame.text })
        } else if (item.port === 'event_out') {
          context.publishNotification?.(item.frame.topic, item.frame.payload)
        }
      }
    }
}

export function defineAgentNode({ createDriver }) {
  if (typeof createDriver !== 'function') throw new TypeError('createDriver must be a function')
  return class MuxivaAgentNode extends AgentTurnController {
    constructor(config = {}) {
      super({ createDriver, config })
    }
  }
}

export class SentenceChunker {
  constructor({ maximumCharacters = 80 } = {}) {
    this.maximumCharacters = positiveInteger(maximumCharacters, 80, 4096)
    this.buffer = ''
  }

  push(delta) {
    this.buffer += String(delta)
    const chunks = []
    const boundaries = new Set(['。', '！', '？', '.', '!', '?', '\n'])
    while (this.buffer.length > 0) {
      let end = [...this.buffer].findIndex((character) => boundaries.has(character))
      if (end >= 0) end += 1
      else if (this.buffer.length >= this.maximumCharacters) end = this.maximumCharacters
      else break
      chunks.push(this.buffer.slice(0, end))
      this.buffer = this.buffer.slice(end)
    }
    return chunks
  }

  flush() {
    if (!this.buffer) return []
    const value = this.buffer
    this.buffer = ''
    return [value]
  }
}
