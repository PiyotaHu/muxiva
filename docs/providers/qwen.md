# Qwen provider setup

The Qwen Provider does **not** require a Qwen or DashScope SDK download. Muxiva
uses the documented WebSocket and HTTP protocols directly. Run:

```sh
./examples/voice-agent/setup.sh
```

This creates `examples/voice-agent/.muxiva/venv` and installs the declared Python
requirements from `providers/algorithm/qwen/python/requirements.txt`. Studio and the CLI
automatically select that project Python environment.

The Provider currently targets Alibaba Cloud Model Studio China (Beijing).
Create the API Key and Workspace ID in the same Workspace and region:

- [Create a Model Studio API Key](https://help.aliyun.com/en/model-studio/get-api-key)
- [Make the first Qwen API call and find the Workspace ID](https://help.aliyun.com/en/model-studio/first-api-call-to-qwen)
- [Qwen Audio Realtime official guide](https://help.aliyun.com/en/model-studio/qwen-audio-realtime-user-guides)
- [Qwen Server VAD + realtime ASR](https://help.aliyun.com/en/model-studio/real-time-speech-recognition-user-guide)
- [Qwen streaming LLM responses](https://help.aliyun.com/en/model-studio/stream)
- [Qwen realtime TTS](https://help.aliyun.com/en/model-studio/interactive-process-of-qwen-tts-realtime-synthesis)

Enter the Key and Workspace ID in Studio **Connections**. Never commit the Key
or expose it to the browser. See the
[complete flagship voice guide](../site/en/voice-demo.md) for the click-by-click
setup and troubleshooting flow.

The flagship **Demo 2** profile uses Qwen ASR Server VAD as its interruption
source, a cancellable background HTTP SSE worker for Qwen LLM, and a reusable
cancellable WebSocket worker for Qwen TTS. The Graph routes one opaque
`muxiva.voice.speech.started` Signal to both workers, stale-output gates, and
Agora playback; Core contains no Alibaba-specific policy.
