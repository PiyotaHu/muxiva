# Official and custom Nodes

Agora and Qwen integrations are ordinary Nodes. Core does not link vendor SDKs or understand
ASR, TTS, RTC, or conversational turns; those semantics stay inside the relevant Nodes.

Official voice Nodes:

- `agora.audio_source`: C++ RTC audio Source with internal scheduling and no Clock Node.
- `agora.audio_sink`: C++ RTC audio Sink that clears playback on `voxa.voice.speech.started`.
- `qwen.audio_realtime`: Python speech-to-speech Node owning VAD, ASR, reasoning, TTS, and late-response cancellation.
- `qwen.asr_realtime`, `qwen.llm_stream`, `qwen.tts_realtime`: replaceable cascade Nodes.

Project Nodes live under `.voxa/nodes/`. Each directory contains `voxa.node.json` and a language
entrypoint; Studio shows its source, registers it, and lets developers drag it into a Graph.
Manifests declare Connection fields while real values stay in the gitignored project `.env`.
