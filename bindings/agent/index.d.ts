export interface AgentCapability {
  id: string
  kind: string
  description?: string
}
export interface AgentRouteDecision {
  id: string
  capabilities: readonly string[]
  requiredCapabilities?: readonly string[]
  reason?: string
  metadata?: Readonly<Record<string, unknown>>
}
export interface AgentPrompt {
  text: string
  sequence: number
  route?: AgentRouteDecision
}
export interface AgentEventSink {
  text(delta: string): void
  event(type: string, payload?: Record<string, unknown>): void
}
export interface AgentDriver {
  run(prompt: AgentPrompt, sink: AgentEventSink, signal: AbortSignal): Promise<void>
  capabilities?(): readonly AgentCapability[]
  route?(prompt: Omit<AgentPrompt, "route">): AgentRouteDecision
  cancel?(reason: unknown): void
  snapshot?(): unknown
  close?(): void | Promise<void>
}
export interface AgentDriverError extends Error {
  /** Stable machine-readable failure class published with response.failed. */
  reason?: string
  /** Optional deployment-safe text to present instead of the generic failure message. */
  userMessage?: string
  /** Set false for bounded tool/business failures that do not require replacing the Driver. */
  recoverDriver?: boolean
}
export interface AgentNodeConfig {
  max_queue_size?: number
  max_results_per_wakeup?: number
  cancel_signals?: string[]
  previous_only_cancel_signals?: string[]
  agent_first_output_timeout_ms?: number
  agent_turn_timeout_ms?: number
  timeout_message?: string
  failure_message?: string
  progress_message?: string
  progress_delay_ms?: number
  [key: string]: unknown
}
export interface AgentNodeDefinition {
  createDriver(options: { config: AgentNodeConfig; state?: unknown }): AgentDriver
}
export interface AgentNodeContext {
  inputPort?: string
  emit(port: string, frame: Record<string, unknown>): void
  publishNotification?(topic: string, payload?: Record<string, unknown>): void
  scheduleNextTick?(delayMs: number): void
}
export interface AgentNodeInstance {
  onProcess(frame: Record<string, unknown>, context: AgentNodeContext): void
  onSignal(signal: Record<string, unknown>): void
  onFinish(): Promise<void>
  onAbort(reason: unknown): void
}
export interface CapabilityRouteDefinition {
  id: string
  capabilities: readonly string[]
  requiredCapabilities?: readonly string[]
  reason?: string
  match(prompt: Readonly<Omit<AgentPrompt, "route">>): boolean | Record<string, unknown>
}
export class CapabilityRouter {
  constructor(options?: {
    capabilities?: readonly AgentCapability[]
    routes?: readonly CapabilityRouteDefinition[]
    fallback?: { id: string; capabilities: readonly string[]; requiredCapabilities?: readonly string[] }
  })
  capabilities(): readonly Readonly<AgentCapability>[]
  route(prompt: Omit<AgentPrompt, "route">): AgentRouteDecision
}
export class AgentTurnController implements AgentNodeInstance {
  constructor(options: { createDriver: AgentNodeDefinition["createDriver"]; config?: AgentNodeConfig })
  onProcess(frame: Record<string, unknown>, context: AgentNodeContext): void
  onSignal(signal: Record<string, unknown>, context?: AgentNodeContext): void
  onFinish(): Promise<void>
  onAbort(reason: unknown): void
}
export type AgentNodeConstructor = new (config?: AgentNodeConfig) => AgentNodeInstance
export function defineAgentNode(definition: AgentNodeDefinition): AgentNodeConstructor
export class SentenceChunker {
  constructor(options?: { maximumCharacters?: number })
  push(delta: string): string[]
  flush(): string[]
}
