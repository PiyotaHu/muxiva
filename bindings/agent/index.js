function positiveInteger(value, fallback, maximum) {
  return Number.isSafeInteger(value) && value > 0 ? Math.min(value, maximum) : fallback
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
 * explicit cancellation, bounded streaming output, and stale-request suppression.
 * It deliberately does not infer conversational Turn semantics from Prompts or
 * Signal sequence numbers. A Voice Turn Controller, when present in the Graph,
 * owns admission and supersession and sends an explicit cancellation Signal.
 */
export class AgentNodeAdapter {
  constructor({ createDriver, config = {} } = {}) {
      if (typeof createDriver !== 'function') throw new TypeError('createDriver must be a function')
      this.config = config
      this.queue = []
      this.nextRequestId = 0
      this.activeController = undefined
      this.activeDriver = undefined
      this.activeRequest = undefined
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
      this.firstOutputTimeoutMs = positiveInteger(
        config.agent_first_output_timeout_ms ?? config.first_output_timeout_ms,
        10_000,
        300_000,
      )
      this.requestTimeoutMs = positiveInteger(
        config.agent_request_timeout_ms ?? config.request_timeout_ms,
        60_000,
        600_000,
      )
      this.timeoutMessage = typeof config.timeout_message === 'string'
        ? config.timeout_message.trim()
        : ''
      this.failureMessage = typeof config.failure_message === 'string'
        ? config.failure_message.trim()
        : ''
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
      this.cancel(signal?.name ?? 'cancelled', Number(signal?.sequence ?? 0))
      if (this.queue.length > 0) context?.scheduleNextTick?.(1)
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
      const request = {
        id: ++this.nextRequestId,
        prompt,
        sequence,
        cancelled: false,
      }
      this.tail = this.tail
        .catch(() => undefined)
        .then(() => this.executeRequest(request))
    }

    cancel(reason, sequence) {
      const controller = this.activeController
      const request = this.activeRequest
      const hadActiveRequest = Boolean(controller && !controller.signal.aborted)
      if (hadActiveRequest) {
        if (request) request.cancelled = true
        controller.abort(reason)
        this.activeDriver?.cancel?.(reason)
      }
      this.activeController = undefined
      this.activeDriver = undefined
      this.activeRequest = undefined
      this.queue.length = 0
      if (hadActiveRequest) {
        this.queue.push({
          port: 'event_out',
          frame: eventFrame('muxiva.agent.response.cancelled', { reason }, sequence),
        })
      }
    }

    async executeRequest(request) {
      const { id, prompt, sequence } = request
      if (request.cancelled || this.closed) return
      if (this.driverBusy) this.rotateDriver('previous_request_unresponsive')
      const controller = new AbortController()
      this.activeController = controller
      this.activeRequest = request
      const driver = this.driver
      const epoch = this.driverEpoch
      this.driverBusy = true
      this.activeDriver = driver
      this.enqueue(request, 'event_out', eventFrame('muxiva.agent.response.started', {}, sequence))
      const startedAt = Date.now()
      let accepting = true
      let responseText = ''
      let firstTextEmitted = false
      let firstOutputTimer
      let requestTimer
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
      requestTimer = setTimeout(
        () => failForTimeout('request', this.requestTimeoutMs),
        this.requestTimeoutMs,
      )
      const sink = {
        text: (delta) => {
          if (!accepting || request.cancelled) return
          const value = String(delta)
          if (!value) return
          clearFirstOutputTimer()
          if (!firstTextEmitted) {
            firstTextEmitted = true
            console.error(`[MUXIVA][AGENT][first_text] sequence=${sequence} duration_ms=${Date.now() - startedAt}`)
          }
          responseText += value
          this.enqueue(request, 'text_out', { kind: 'text', text: value, sequence })
        },
        event: (type, payload = {}) => {
          if (!accepting || request.cancelled) return
          if (type === 'tool.started') {
            clearFirstOutputTimer()
            console.error(`[MUXIVA][AGENT][first_activity] sequence=${sequence} type=tool.started duration_ms=${Date.now() - startedAt}`)
          }
          this.enqueue(request, 'event_out', eventFrame(`muxiva.agent.${type}`, payload, sequence))
        },
      }
      let removeAbortListener = () => undefined
      const aborted = new Promise((_, reject) => {
        const onAbort = () => reject(controller.signal.reason ?? new Error('Agent request cancelled'))
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
      console.error(`[MUXIVA][AGENT][request.started] sequence=${sequence} request_id=${id} driver_epoch=${epoch}`)
      try {
        await Promise.race([driverRun, aborted, watchdog])
        if (!controller.signal.aborted && !request.cancelled) {
          this.enqueue(
            request,
            'event_out',
            eventFrame('muxiva.agent.response.completed', { text: responseText }, sequence),
          )
          console.error(`[MUXIVA][AGENT][request.completed] sequence=${sequence} duration_ms=${Date.now() - startedAt}`)
        }
      } catch (error) {
        const timedOut = error?.code === 'MUXIVA_AGENT_FIRST_OUTPUT_TIMEOUT'
          || error?.code === 'MUXIVA_AGENT_REQUEST_TIMEOUT'
        if (timedOut && !request.cancelled) {
          accepting = false
          this.rotateDriver(`timeout.${error.stage}`)
          this.enqueue(
            request,
            'event_out',
            eventFrame('muxiva.agent.response.failed', {
              message: error.message,
              reason: `timeout.${error.stage}`,
              duration_ms: Date.now() - startedAt,
            }, sequence),
          )
          if (this.timeoutMessage) {
            this.enqueue(request, 'text_out', { kind: 'text', text: this.timeoutMessage, sequence })
          }
          console.error(`[MUXIVA][AGENT][request.timeout] sequence=${sequence} stage=${error.stage} duration_ms=${Date.now() - startedAt}`)
        } else if (!controller.signal.aborted && !request.cancelled) {
          accepting = false
          const recoverDriver = !(error && typeof error === 'object' && error.recoverDriver === false)
          const reason = errorString(error, 'reason') || errorString(error, 'code') || 'run.failed'
          const userMessage = errorString(error, 'userMessage') || this.failureMessage
          if (recoverDriver) this.rotateDriver(reason)
          console.error('[MUXIVA][AGENT][response.failed]', error?.stack ?? error?.message ?? String(error))
          this.enqueue(
            request,
            'event_out',
            eventFrame('muxiva.agent.response.failed', {
              message: error?.message ?? String(error),
              reason,
              driver_recovered: recoverDriver,
            }, sequence),
          )
          if (userMessage) {
            this.enqueue(request, 'text_out', { kind: 'text', text: userMessage, sequence })
          }
        }
      } finally {
        accepting = false
        removeAbortListener()
        if (firstOutputTimer) clearTimeout(firstOutputTimer)
        if (requestTimer) clearTimeout(requestTimer)
        if (this.activeController === controller) {
          this.activeController = undefined
          this.activeDriver = undefined
          this.activeRequest = undefined
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

    enqueue(request, port, frame) {
      if (request.cancelled || this.closed) return
      if (this.queue.length >= this.maxQueueSize) {
        request.cancelled = true
        this.activeController?.abort('output.queue_full')
        this.activeDriver?.cancel?.('output.queue_full')
        this.queue.length = 0
        this.queue.push({
          port: 'event_out',
          frame: eventFrame(
            'muxiva.agent.response.failed',
            { message: `Agent output queue exceeded ${this.maxQueueSize} items` },
            frame.sequence ?? 0,
          ),
        })
        return
      }
      this.queue.push({ port, frame })
    }

    drain(context) {
      for (let index = 0; index < this.maxResultsPerWakeup; index += 1) {
        const item = this.queue.shift()
        if (!item) return
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
  return class MuxivaAgentNode extends AgentNodeAdapter {
    constructor(config = {}) {
      super({ createDriver, config })
    }
  }
}
