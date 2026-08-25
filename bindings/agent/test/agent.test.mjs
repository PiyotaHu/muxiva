import assert from 'node:assert/strict'
import test from 'node:test'
import { AgentNodeAdapter, CapabilityRouter, defineAgentNode } from '../index.js'

test('Agent Node streams semantic output and suppresses stale work after cancellation', async () => {
  let release
  const Node = defineAgentNode({
    createDriver() {
      return {
        async run(_prompt, sink) {
          sink.text('first')
          await new Promise((resolve) => { release = resolve })
          sink.text('stale')
        },
        cancel() {},
      }
    },
  })
  const node = new Node({ max_results_per_wakeup: 32 })
  const scheduled = []
  node.onProcess(
    { kind: 'text', text: 'hello', sequence: 7 },
    { inputPort: 'prompt_in', scheduleNextTick: (delay) => scheduled.push(delay) },
  )
  await new Promise((resolve) => setImmediate(resolve))
  node.onSignal(
    { name: 'application.stop_requested', sequence: 8 },
    { scheduleNextTick: (delay) => scheduled.push(delay) },
  )
  release()
  await new Promise((resolve) => setImmediate(resolve))
  const output = []
  node.onProcess(
    undefined,
    { inputPort: undefined, emit: (port, frame) => output.push({ port, frame }) },
  )
  assert.deepEqual(scheduled, [20, 1])
  assert.equal(output.some(({ frame }) => frame.text === 'stale'), false)
  assert.equal(output.at(-1).frame.topic, 'muxiva.agent.response.cancelled')
})

test('a new Prompt queues without implying cancellation or supersession', async () => {
  let releaseFirst
  const requests = []
  const Node = defineAgentNode({
    createDriver() {
      return {
        async run(prompt, sink) {
          requests.push(prompt.text)
          if (prompt.text === 'first') {
            await new Promise((resolve) => { releaseFirst = resolve })
          }
          sink.text(prompt.text)
        },
      }
    },
  })
  const node = new Node()
  const context = { inputPort: 'prompt_in', scheduleNextTick() {} }
  node.onProcess(
    { kind: 'text', text: 'first', sequence: 41 },
    context,
  )
  await new Promise((resolve) => setImmediate(resolve))
  node.onProcess({ kind: 'text', text: 'second', sequence: 42 }, context)
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(requests, ['first'])
  releaseFirst()
  await new Promise((resolve) => setImmediate(resolve))
  await new Promise((resolve) => setImmediate(resolve))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.deepEqual(requests, ['first', 'second'])
  assert.deepEqual(output.filter(({ frame }) => frame.kind === 'text').map(({ frame }) => frame.text), ['first', 'second'])
  assert.equal(output.some(({ frame }) => frame.topic === 'muxiva.agent.response.cancelled'), false)
})

test('a cancelled driver that ignores abort cannot block the next request', async () => {
  let creations = 0
  const Node = defineAgentNode({
    createDriver() {
      const id = ++creations
      return {
        async run(_prompt, sink) {
          if (id === 1) await new Promise(() => undefined)
          else sink.text('second request works')
        },
        cancel() {},
      }
    },
  })
  const node = new Node()
  const context = { inputPort: 'prompt_in', scheduleNextTick() {} }
  node.onProcess({ kind: 'text', text: 'first', sequence: 10 }, context)
  await new Promise((resolve) => setImmediate(resolve))
  node.onProcess({ kind: 'text', text: 'second', sequence: 12 }, context)
  node.onSignal({ name: 'muxiva.turn.cancelled', sequence: 11 }, { scheduleNextTick() {} })
  await new Promise((resolve) => setImmediate(resolve))
  await new Promise((resolve) => setImmediate(resolve))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(creations, 2)
  assert.equal(output.some(({ frame }) => frame.text === 'second request works'), true)
  assert.equal(output.some(({ frame }) => frame.text === 'stale'), false)
})

test('first-output watchdog fails visibly and rotates the driver', async () => {
  let creations = 0
  const Node = defineAgentNode({
    createDriver() {
      creations += 1
      return {
        async run() { await new Promise(() => undefined) },
        cancel() {},
      }
    },
  })
  const node = new Node({
    agent_first_output_timeout_ms: 20,
    agent_request_timeout_ms: 100,
    timeout_message: '已经自动恢复',
  })
  node.onProcess(
    { kind: 'text', text: 'hang', sequence: 20 },
    { inputPort: 'prompt_in', scheduleNextTick() {} },
  )
  await new Promise((resolve) => setTimeout(resolve, 40))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(creations, 2)
  assert.equal(output.some(({ frame }) => frame.topic === 'muxiva.agent.response.failed'), true)
  assert.equal(output.some(({ frame }) => frame.text === '已经自动恢复'), true)
})

test('bounded tool failures keep the driver and present their own safe message', async () => {
  let creations = 0
  const Node = defineAgentNode({
    createDriver() {
      creations += 1
      return {
        async run() {
          const error = new Error('upstream returned HTTP 503')
          error.reason = 'tool.artwork.failed'
          error.userMessage = '这次画图没有完成，请稍后再试。'
          error.recoverDriver = false
          throw error
        },
      }
    },
  })
  const node = new Node({ failure_message: '模型连接失败' })
  node.onProcess(
    { kind: 'text', text: 'draw', sequence: 22 },
    { inputPort: 'prompt_in', scheduleNextTick() {} },
  )
  await new Promise((resolve) => setImmediate(resolve))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  const failure = output.find(({ frame }) => frame.topic === 'muxiva.agent.response.failed')
  assert.equal(creations, 1)
  assert.equal(failure.frame.payload.reason, 'tool.artwork.failed')
  assert.equal(failure.frame.payload.driver_recovered, false)
  assert.equal(output.some(({ frame }) => frame.text === '这次画图没有完成，请稍后再试。'), true)
  assert.equal(output.some(({ frame }) => frame.text === '模型连接失败'), false)
})

test('driver rotation transfers an optional state snapshot to the replacement', async () => {
  const receivedStates = []
  let creations = 0
  const Node = defineAgentNode({
    createDriver({ state }) {
      const id = ++creations
      receivedStates.push(state)
      return {
        async run(_prompt, sink) {
          if (id === 1) await new Promise(() => undefined)
          else sink.text(`history:${state.history}`)
        },
        snapshot: () => ({ history: 3 }),
        cancel() {},
      }
    },
  })
  const node = new Node()
  const context = { inputPort: 'prompt_in', scheduleNextTick() {} }
  node.onProcess({ kind: 'text', text: 'first', sequence: 21 }, context)
  await new Promise((resolve) => setImmediate(resolve))
  node.onSignal({ name: 'muxiva.turn.cancelled', sequence: 22 }, { scheduleNextTick() {} })
  node.onProcess({ kind: 'text', text: 'second', sequence: 23 }, context)
  await new Promise((resolve) => setImmediate(resolve))
  await new Promise((resolve) => setImmediate(resolve))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.deepEqual(receivedStates, [undefined, { history: 3 }])
  assert.equal(output.some(({ frame }) => frame.text === 'history:3'), true)
})

test('CapabilityRouter validates declarations and keeps business matchers outside the framework', () => {
  const router = new CapabilityRouter({
    capabilities: [
      { id: 'model.chat', kind: 'model' },
      { id: 'tool.web_search', kind: 'tool' },
    ],
    routes: [{
      id: 'live_information',
      capabilities: ['model.chat', 'tool.web_search'],
      reason: 'application policy',
      match: ({ text }) => text.includes('latest') && { intent: 'latest' },
    }],
    fallback: { id: 'fast_chat', capabilities: ['model.chat'] },
  })
  assert.deepEqual(router.route({ text: 'latest news', sequence: 1 }), {
    id: 'live_information',
    capabilities: ['model.chat', 'tool.web_search'],
    requiredCapabilities: [],
    reason: 'application policy',
    metadata: { intent: 'latest' },
  })
  assert.equal(router.route({ text: 'why fruit smells', sequence: 2 }).id, 'fast_chat')
  assert.throws(() => new CapabilityRouter({
    routes: [{ id: 'bad', capabilities: ['tool.missing'], match: () => true }],
  }), /unknown capability/)
  assert.throws(() => new CapabilityRouter({
    capabilities: [{ id: 'model.chat', kind: 'model' }],
    routes: [{
      id: 'bad_required', capabilities: ['model.chat'],
      requiredCapabilities: ['tool.web_search'], match: () => true,
    }],
  }), /requires capability not granted/)
})

test('AgentNodeAdapter emits a validated route decision and passes it to the driver', async () => {
  let receivedPrompt
  const adapter = new AgentNodeAdapter({
    createDriver() {
      return {
        capabilities: () => [{ id: 'model.chat', kind: 'model' }],
        route: () => ({
          id: 'fast_chat', capabilities: ['model.chat'],
          requiredCapabilities: ['model.chat'], reason: 'test',
        }),
        async run(prompt, sink) {
          receivedPrompt = prompt
          sink.text('ok')
        },
      }
    },
  })
  adapter.onProcess(
    { kind: 'text', text: 'hello', sequence: 30 },
    { inputPort: 'prompt_in', scheduleNextTick() {} },
  )
  await new Promise((resolve) => setImmediate(resolve))
  const output = []
  adapter.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(receivedPrompt.route.id, 'fast_chat')
  assert.deepEqual(receivedPrompt.route.requiredCapabilities, ['model.chat'])
  assert.equal(output.some(({ frame }) => frame.topic === 'muxiva.agent.route.selected'), true)
})
