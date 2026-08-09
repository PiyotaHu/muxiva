import assert from 'node:assert/strict'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

test('Pi Agent Node loads as a project TypeScript Node without contacting a provider', async () => {
  process.env.DASHSCOPE_API_KEY = 'test-only-key'
  process.env.DASHSCOPE_WORKSPACE_ID = 'test-workspace'
  process.env.MUXIVA_PROJECT_ROOT = fileURLToPath(new URL('..', import.meta.url))
  const { PiAgentNode } = await import('../.muxiva/nodes/pi_agent/node.ts')
  const node = new PiAgentNode({
    model: 'qwen-flash',
    system_prompt: 'Test prompt',
  })
  assert.equal(typeof node.onProcess, 'function')
  assert.equal(typeof node.onSignal, 'function')
  await node.onFinish()
})
