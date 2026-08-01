import assert from 'node:assert/strict'
import test from 'node:test'
import { EventBus, Runtime, TextFrame } from '../index.js'

test('Runtime and Session close exactly once', () => {
  const runtime = new Runtime()
  const session = runtime.createSession()
  assert.equal(session.isClosed, false)
  assert.equal(session.close(), true)
  assert.equal(session.close(), false)
  assert.equal(runtime.close(), true)
  assert.throws(() => runtime.createSession(), /closed/)
})

test('text frames own their input and expose immutable values', () => {
  const frame = new TextFrame('hello', 7)
  assert.equal(frame.text, 'hello')
  assert.equal(frame.sequence, 7)
  assert.equal(frame.asFrame().kind, 'text')
  assert.throws(() => { frame.text = 'changed' })
})

test('EventBus schedules subscribers without inline invocation', async () => {
  const bus = new EventBus()
  let seen
  bus.subscribe('test.topic', (payload) => { seen = payload }, 1)
  assert.equal(bus.publish('test.topic', '{"ok":true}'), 1)
  assert.equal(bus.publish('test.topic', '{"queued":true}'), 0)
  assert.equal(seen, undefined)
  await new Promise(setImmediate)
  assert.equal(seen, '{"ok":true}')
  assert.equal(bus.publish('test.topic', '{"afterDrain":true}'), 1)
  await new Promise(setImmediate)
  assert.equal(seen, '{"afterDrain":true}')
  assert.equal(bus.close(), true)
})
