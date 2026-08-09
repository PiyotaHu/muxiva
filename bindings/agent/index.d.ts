export interface AgentPrompt { text: string; sequence: number }
export interface AgentEventSink {
  text(delta: string): void
  event(type: string, payload?: Record<string, unknown>): void
}
export interface AgentDriver {
  run(prompt: AgentPrompt, sink: AgentEventSink, signal: AbortSignal): Promise<void>
  cancel?(reason: unknown): void
  close?(): void | Promise<void>
}
export interface AgentNodeConfig {
  max_queue_size?: number
  max_results_per_tick?: number
  cancel_signals?: string[]
  [key: string]: unknown
}
export interface AgentNodeDefinition {
  createDriver(options: { config: AgentNodeConfig }): AgentDriver
}
export interface AgentNodeContext {
  inputPort?: string
  emit(port: string, frame: Record<string, unknown>): void
  publishNotification?(topic: string, payload?: Record<string, unknown>): void
}
export interface AgentNodeInstance {
  onProcess(frame: Record<string, unknown>, context: AgentNodeContext): void
  onSignal(signal: Record<string, unknown>): void
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
