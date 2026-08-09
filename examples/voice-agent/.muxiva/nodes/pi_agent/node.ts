import { defineAgentNode } from '@muxiva/agent'
import { createMuxivaPiAgentDriver } from '@piyotahu/muxiva-pi-agent'

// This file is deliberately only an integration adapter. The application-owned
// Agent, tools, model session, and permission policy live in the independently
// versioned PiyotaHu/muxiva-pi-agent repository.
export const PiAgentNode = defineAgentNode({
  createDriver: createMuxivaPiAgentDriver,
})
