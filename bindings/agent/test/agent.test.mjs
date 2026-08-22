import assert from 'node:assert/strict'
import test from 'node:test'
import { AgentTurnController, CapabilityRouter, defineAgentNode, SentenceChunker } from '../index.js'

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
    { name: 'muxiva.voice.speech.started', sequence: 8 },
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

test('a same-sequence speech Signal cannot cancel the Prompt it accompanies', async () => {
  let release
  let cancelled = false
  const Node = defineAgentNode({
    createDriver() {
      return {
        async run(_prompt, sink) {
          await new Promise((resolve) => { release = resolve })
          sink.text('same turn survived')
        },
        cancel() { cancelled = true },
      }
    },
  })
  const node = new Node()
  node.onProcess(
    { kind: 'text', text: 'new prompt', sequence: 42 },
    { inputPort: 'prompt_in', scheduleNextTick() {} },
  )
  await new Promise((resolve) => setImmediate(resolve))
  node.onSignal(
    { name: 'muxiva.voice.speech.started', sequence: 42 },
    { scheduleNextTick() {} },
  )
  release()
  await new Promise((resolve) => setImmediate(resolve))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(cancelled, false)
  assert.equal(output.some(({ frame }) => frame.text === 'same turn survived'), true)
  assert.equal(output.some(({ frame }) => frame.topic === 'muxiva.agent.response.cancelled'), false)
})

test('SentenceChunker emits speech-sized chunks and flushes its remainder', () => {
  const chunker = new SentenceChunker({ maximumCharacters: 8 })
  assert.deepEqual(chunker.push('你好。继续'), ['你好。'])
  assert.deepEqual(chunker.flush(), ['继续'])
})

test('a cancelled driver that ignores abort cannot block the next turn', async () => {
  let creations = 0
  const Node = defineAgentNode({
    createDriver() {
      const id = ++creations
      return {
        async run(_prompt, sink) {
          if (id === 1) await new Promise(() => undefined)
          else sink.text('second turn works')
        },
        cancel() {},
      }
    },
  })
  const node = new Node()
  const context = { inputPort: 'prompt_in', scheduleNextTick() {} }
  node.onProcess({ kind: 'text', text: 'first', sequence: 10 }, context)
  await new Promise((resolve) => setImmediate(resolve))
  node.onSignal({ name: 'muxiva.voice.speech.started', sequence: 11 }, { scheduleNextTick() {} })
  node.onProcess({ kind: 'text', text: 'second', sequence: 12 }, context)
  await new Promise((resolve) => setImmediate(resolve))
  await new Promise((resolve) => setImmediate(resolve))

  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(creations, 2)
  assert.equal(output.some(({ frame }) => frame.text === 'second turn works'), true)
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
    agent_turn_timeout_ms: 100,
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
  node.onSignal({ name: 'muxiva.voice.speech.started', sequence: 22 }, { scheduleNextTick() {} })
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

test('AgentTurnController emits a validated route decision and passes it to the driver', async () => {
  let receivedPrompt
  const controller = new AgentTurnController({
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
  controller.onProcess(
    { kind: 'text', text: 'hello', sequence: 30 },
    { inputPort: 'prompt_in', scheduleNextTick() {} },
  )
  await new Promise((resolve) => setImmediate(resolve))
  const output = []
  controller.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(receivedPrompt.route.id, 'fast_chat')
  assert.deepEqual(receivedPrompt.route.requiredCapabilities, ['model.chat'])
  assert.equal(output.some(({ frame }) => frame.topic === 'muxiva.agent.route.selected'), true)
})

test('progress speech is opt-in and delay zero disables it', async () => {
  const Node = defineAgentNode({
    createDriver() {
      return { async run(_prompt, sink) {
        sink.event('tool.started', { name: 'slow_tool' })
        await new Promise((resolve) => setTimeout(resolve, 30))
        sink.text('完成')
      } }
    },
  })
  const node = new Node({ progress_message: '处理中', progress_delay_ms: 0 })
  node.onProcess(
    { kind: 'text', text: 'go', sequence: 40 },
    { inputPort: 'prompt_in', scheduleNextTick() {} },
  )
  await new Promise((resolve) => setTimeout(resolve, 50))
  const output = []
  node.onProcess(undefined, {
    inputPort: undefined,
    emit: (port, frame) => output.push({ port, frame }),
  })
  assert.equal(output.some(({ frame }) => frame.text === '处理中'), false)
  assert.equal(output.some(({ frame }) => frame.text === '完成'), true)
})
