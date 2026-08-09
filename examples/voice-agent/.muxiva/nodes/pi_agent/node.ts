import {
  Agent,
  type AgentEvent,
  type AgentTool,
} from '@earendil-works/pi-agent-core'
import {
  createModels,
  createProvider,
  envApiKeyAuth,
  type Model,
} from '@earendil-works/pi-ai'
import { openAICompletionsApi } from '@earendil-works/pi-ai/api/openai-completions.lazy'
import {
  defineAgentNode,
  SentenceChunker,
  type AgentDriver,
  type AgentEventSink,
  type AgentNodeConfig,
  type AgentPrompt,
} from '@muxiva/agent'
import { Type } from 'typebox'

function textConfig(config: AgentNodeConfig, name: string, fallback: string): string {
  const value = config[name]
  return typeof value === 'string' && value.trim() ? value : fallback
}

function numberConfig(config: AgentNodeConfig, name: string, fallback: number): number {
  const value = config[name]
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function dashScopeModel(config: AgentNodeConfig): Model<'openai-completions'> {
  const workspace = process.env.DASHSCOPE_WORKSPACE_ID?.trim()
  if (!workspace || !/^[A-Za-z0-9-]{1,128}$/.test(workspace)) {
    throw new Error('configure a valid DASHSCOPE_WORKSPACE_ID in Studio Connections')
  }
  const model = textConfig(config, 'model', 'qwen-flash')
  return {
    id: model,
    name: `${model} through Pi`,
    api: 'openai-completions',
    provider: 'dashscope',
    baseUrl: `https://${workspace}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`,
    reasoning: false,
    input: ['text'],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 1_000_000,
    maxTokens: numberConfig(config, 'max_tokens', 2048),
    compat: {
      supportsDeveloperRole: false,
      supportsReasoningEffort: false,
      supportsStrictMode: false,
      maxTokensField: 'max_tokens',
      thinkingFormat: 'qwen',
    },
  }
}

const currentTimeParameters = Type.Object({
  time_zone: Type.Optional(Type.String({ description: 'IANA time zone, for example Asia/Shanghai' })),
})

const currentTimeTool: AgentTool<typeof currentTimeParameters, Record<string, unknown>> = {
  name: 'get_current_time',
  label: 'Get current time',
  description: 'Get the current date and time in an IANA time zone.',
  parameters: currentTimeParameters,
  async execute(_callId, parameters) {
    const timeZone = parameters.time_zone || 'Asia/Shanghai'
    const formatted = new Intl.DateTimeFormat('zh-CN', {
      dateStyle: 'full', timeStyle: 'long', timeZone,
    }).format(new Date())
    return {
      content: [{ type: 'text', text: `${timeZone}: ${formatted}` }],
      details: { time_zone: timeZone, formatted },
    }
  },
}

const weatherParameters = Type.Object({
  city: Type.String({ description: 'City name in the user language' }),
})

const weatherTool: AgentTool<typeof weatherParameters, Record<string, unknown>> = {
  name: 'get_current_weather',
  label: 'Get current weather',
  description: 'Look up live current weather for a city. Use this instead of guessing current weather.',
  parameters: weatherParameters,
  async execute(_callId, parameters, signal) {
    const geocodeUrl = new URL('https://geocoding-api.open-meteo.com/v1/search')
    geocodeUrl.searchParams.set('name', parameters.city)
    geocodeUrl.searchParams.set('count', '1')
    geocodeUrl.searchParams.set('language', 'zh')
    const geocode = await fetch(geocodeUrl, { signal }).then(async (response) => {
      if (!response.ok) throw new Error(`weather geocoding failed with HTTP ${response.status}`)
      return await response.json() as { results?: Array<{ name: string; country?: string; latitude: number; longitude: number }> }
    })
    const location = geocode.results?.[0]
    if (!location) throw new Error(`city not found: ${parameters.city}`)

    const forecastUrl = new URL('https://api.open-meteo.com/v1/forecast')
    forecastUrl.searchParams.set('latitude', String(location.latitude))
    forecastUrl.searchParams.set('longitude', String(location.longitude))
    forecastUrl.searchParams.set('current', 'temperature_2m,apparent_temperature,weather_code,wind_speed_10m')
    forecastUrl.searchParams.set('timezone', 'auto')
    const forecast = await fetch(forecastUrl, { signal }).then(async (response) => {
      if (!response.ok) throw new Error(`weather lookup failed with HTTP ${response.status}`)
      return await response.json() as {
        current?: { temperature_2m?: number; apparent_temperature?: number; weather_code?: number; wind_speed_10m?: number }
        current_units?: Record<string, string>
      }
    })
    const current = forecast.current
    if (!current) throw new Error('weather service returned no current conditions')
    const details = {
      city: location.name,
      country: location.country,
      temperature: current.temperature_2m,
      apparent_temperature: current.apparent_temperature,
      weather_code: current.weather_code,
      wind_speed: current.wind_speed_10m,
      units: forecast.current_units,
      source: 'Open-Meteo.com',
    }
    return {
      content: [{ type: 'text', text: JSON.stringify(details) }],
      details,
    }
  },
}

class PiDriver implements AgentDriver {
  private readonly agent: Agent
  private readonly temperature: number
  private readonly chunkCharacters: number
  private unsubscribe: (() => void) | undefined
  private sink: AgentEventSink | undefined
  private chunker: SentenceChunker | undefined

  constructor(config: AgentNodeConfig) {
    if (!process.env.DASHSCOPE_API_KEY?.trim()) {
      throw new Error('configure DASHSCOPE_API_KEY in Studio Connections')
    }
    const model = dashScopeModel(config)
    const provider = createProvider({
      id: 'dashscope',
      name: 'Alibaba Cloud Model Studio',
      baseUrl: model.baseUrl,
      auth: { apiKey: envApiKeyAuth('DashScope API key', ['DASHSCOPE_API_KEY']) },
      models: [model],
      api: openAICompletionsApi(),
    })
    const models = createModels()
    models.setProvider(provider)
    this.temperature = numberConfig(config, 'temperature', 0.6)
    this.chunkCharacters = numberConfig(config, 'sentence_chunk_characters', 80)
    this.agent = new Agent({
      initialState: {
        systemPrompt: textConfig(
          config,
          'system_prompt',
          "You are Muxiva, a warm, concise real-time voice agent. Use tools whenever current facts are needed. Respond in the user's language.",
        ),
        model,
        thinkingLevel: 'off',
        tools: [currentTimeTool, weatherTool],
      },
      streamFn: (activeModel, context, options) => models.streamSimple(activeModel, context, {
        ...options,
        temperature: this.temperature,
      }),
      toolExecution: 'parallel',
    })
    this.unsubscribe = this.agent.subscribe((event) => this.onAgentEvent(event))
  }

  async run(_prompt: AgentPrompt, sink: AgentEventSink, signal: AbortSignal): Promise<void> {
    this.sink = sink
    this.chunker = new SentenceChunker({ maximumCharacters: this.chunkCharacters })
    const abort = () => this.agent.abort()
    signal.addEventListener('abort', abort, { once: true })
    try {
      await this.agent.prompt(_prompt.text)
      for (const chunk of this.chunker.flush()) sink.text(chunk)
      if (this.agent.state.errorMessage && !signal.aborted) throw new Error(this.agent.state.errorMessage)
    } finally {
      signal.removeEventListener('abort', abort)
      this.sink = undefined
      this.chunker = undefined
    }
  }

  cancel(): void {
    this.agent.abort()
  }

  async close(): Promise<void> {
    this.agent.abort()
    await this.agent.waitForIdle()
    this.unsubscribe?.()
    this.unsubscribe = undefined
  }

  private onAgentEvent(event: AgentEvent): void {
    const sink = this.sink
    if (!sink) return
    if (event.type === 'message_update' && event.assistantMessageEvent.type === 'text_delta') {
      for (const chunk of this.chunker?.push(event.assistantMessageEvent.delta) ?? []) sink.text(chunk)
    } else if (event.type === 'tool_execution_start') {
      sink.event('tool.started', { id: event.toolCallId, name: event.toolName, arguments: event.args })
    } else if (event.type === 'tool_execution_update') {
      sink.event('tool.updated', { id: event.toolCallId, name: event.toolName })
    } else if (event.type === 'tool_execution_end') {
      sink.event('tool.completed', { id: event.toolCallId, name: event.toolName, error: event.isError })
    } else if (event.type === 'turn_start') {
      sink.event('turn.started')
    } else if (event.type === 'turn_end') {
      sink.event('turn.completed', { tool_results: event.toolResults.length })
    }
  }
}

export const PiAgentNode = defineAgentNode({
  createDriver({ config }) {
    return new PiDriver(config)
  },
})
