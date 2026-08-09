const DEFAULT_CANCEL_SIGNALS = ['muxiva.agent.cancel', 'muxiva.voice.speech.started']

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

/**
 * Wrap a vendor-specific Agent driver in Muxiva's stable Agent Node contract.
 * The driver owns model/session/tool behavior; this adapter owns graph lifecycle,
 * cancellation, bounded streaming output, and stale-generation suppression.
 */
export function defineAgentNode({ createDriver }) {
  if (typeof createDriver !== 'function') throw new TypeError('createDriver must be a function')

  return class MuxivaAgentNode {
    constructor(config = {}) {
      this.config = config
      this.queue = []
      this.generation = 0
      this.activeController = undefined
      this.closed = false
      this.tail = Promise.resolve()
      this.maxQueueSize = positiveInteger(config.max_queue_size, 2048, 65536)
      this.maxResultsPerTick = positiveInteger(config.max_results_per_tick, 64, 1024)
      this.cancelSignals = new Set(
        Array.isArray(config.cancel_signals) ? config.cancel_signals : DEFAULT_CANCEL_SIGNALS,
      )
      this.driver = createDriver({ config })
      if (!this.driver || typeof this.driver.run !== 'function') {
        throw new TypeError('createDriver must return an object with run(prompt, sink, signal)')
      }
    }

    onProcess(frame, context) {
      if (context.inputPort === 'prompt_in') {
        if (frame?.kind !== 'text') throw new TypeError('prompt_in requires a TextFrame')
        this.start(frame.text, frame.sequence ?? 0)
        return
      }
      if (context.inputPort === 'tick_in') {
        this.drain(context)
        return
      }
      throw new Error(`Agent Node received unsupported input Port: ${String(context.inputPort)}`)
    }

    onSignal(signal) {
      if (this.cancelSignals.has(signal?.name)) this.cancel(signal?.name ?? 'cancelled', signal?.sequence ?? 0)
    }

    async onFinish() {
      this.closed = true
      this.cancel('runtime.finished', 0)
      await this.tail.catch(() => undefined)
      await this.driver.close?.()
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
      this.tail = this.tail
        .catch(() => undefined)
        .then(async () => {
          if (generation !== this.generation || controller.signal.aborted || this.closed) return
          this.enqueue(generation, 'event_out', eventFrame('muxiva.agent.response.started', {}, sequence))
          let responseText = ''
          const sink = {
            text: (delta) => {
              const value = String(delta)
              if (!value) return
              responseText += value
              this.enqueue(generation, 'text_out', { kind: 'text', text: value, sequence })
            },
            event: (type, payload = {}) => {
              this.enqueue(generation, 'event_out', eventFrame(`muxiva.agent.${type}`, payload, sequence))
            },
          }
          try {
            await this.driver.run({ text: prompt, sequence }, sink, controller.signal)
            if (!controller.signal.aborted && generation === this.generation) {
              this.enqueue(
                generation,
                'event_out',
                eventFrame('muxiva.agent.response.completed', { text: responseText }, sequence),
              )
            }
          } catch (error) {
            if (!controller.signal.aborted && generation === this.generation) {
              this.enqueue(
                generation,
                'event_out',
                eventFrame('muxiva.agent.response.failed', { message: error?.message ?? String(error) }, sequence),
              )
            }
          } finally {
            if (generation === this.generation) this.activeController = undefined
          }
        })
    }

    cancel(reason, sequence) {
      const controller = this.activeController
      if (!controller || controller.signal.aborted) return
      controller.abort(reason)
      this.driver.cancel?.(reason)
      this.activeController = undefined
      this.generation += 1
      this.queue.length = 0
      this.queue.push({
        generation: this.generation,
        port: 'event_out',
        frame: eventFrame('muxiva.agent.response.cancelled', { reason }, sequence),
      })
    }

    enqueue(generation, port, frame) {
      if (generation !== this.generation || this.closed) return
      if (this.queue.length >= this.maxQueueSize) {
        this.activeController?.abort('output.queue_full')
        this.driver.cancel?.('output.queue_full')
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
      for (let index = 0; index < this.maxResultsPerTick; index += 1) {
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
