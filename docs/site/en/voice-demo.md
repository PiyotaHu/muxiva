# Flagship voice demo

This is Voxa's real voice experience, not a mock. The browser captures the
microphone and plays responses, Agora Native C++ Node Packs own bidirectional
RTC audio, Qwen Python Node Packs own intelligence, and the Rust Runtime owns
only vendor-neutral Frames, Graphs, Signals, EventBus, turns, and scheduling.

## Choose a graph

| Graph | Best for | Pipeline |
| --- | --- | --- |
| **Qwen Realtime (recommended)** | First run and low-latency conversation | Audio → Qwen Audio Realtime → Audio |
| **Qwen Cascade** | Inspecting and replacing each intelligence stage | VAD → ASR → LLM → TTS |

Both graphs stay live until you end the session. When the cascade detects new
speech, it emits `voxa.runtime.interrupt`. The Runtime advances the global turn
and rejects previous-turn audio immediately before the speaker Sink.

## 1. Prepare the environment

```bash
git clone https://github.com/PiyotaHu/Voxa.git
cd Voxa
cargo install --locked --path crates/voxa-cli
voxa doctor --voice
```

You also need:

- an Agora Native C++ SDK for the current platform;
- an Alibaba Cloud Model Studio DashScope API Key and Workspace ID;
- one Agora App ID and channel;
- short-lived tokens for three distinct UIDs in that channel.

| Identity | Purpose | Token capability |
| --- | --- | --- |
| Browser | Browser microphone and speaker | Publish and subscribe |
| Ingress bot | C++ receives browser audio | Subscribe |
| Egress bot | C++ publishes assistant audio | Publish |

Never place an Agora App Certificate in the repository, Studio, or browser.

## 2. Install the application Node Packs

```bash
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
voxa doctor --voice
```

This installs Qwen Python Node dependencies and builds both Agora C++ Node
Packs into the project's `.voxa/native/` directory. A successful run ends with
`[VOXA][READY]`. `doctor` reports only whether credentials are configured; it
never prints secret values.

## 3. Start Studio

```bash
./examples/voice-agent/run.sh
```

Studio opens locally. Follow this order:

1. **Templates** → choose **Qwen Realtime**;
2. **Connections** → fill the DashScope and Agora fields;
3. **Voice Room** → open the voice experience;
4. **Start live conversation** → grant microphone access;
5. speak naturally and interrupt while the assistant is talking;
6. select **End session** when finished.

Voice Room shows the microphone waveform, user transcript, assistant response,
Graph stages, Node calls, and Frame activity. After Realtime works, return to
Studio and switch to Cascade to compare the two pipelines.

## Credential boundary

The DashScope Key, Workspace ID, and bot tokens remain in the local Studio
process. The browser receives only Manifest-approved App ID, channel, browser
UID, and short-lived browser token fields. Never commit credentials to Git.

## Verify the development environment

You can verify code, ABI loading, and graph templates without real credentials:

```bash
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

These commands are engineering gates, not a simulated call. Full acceptance
requires joining the Agora channel, speaking into a real microphone, hearing a
Qwen response, and successfully barging in during playback.
