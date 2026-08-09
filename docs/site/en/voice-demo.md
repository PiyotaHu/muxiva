# Run the real voice agent from scratch

This guide starts from a clean macOS development environment and assumes no
prior Agora or Qwen knowledge. At the end, browser microphone audio travels
through Agora into a Muxiva Graph, Qwen generates a live response, and Agora
plays it back.

!!! danger "An App ID alone cannot run the demo"
    Prepare two Agora RTC tokens, a Model Studio API key, and a Workspace ID before selecting
    Run or Voice Room. Follow the [field-by-field credential checklist](voice-credentials.md).

!!! info "What you actually need"
    Agora requires an account, App ID, and two temporary RTC tokens. Qwen
    requires **no SDK download**—only an Alibaba Cloud Model Studio API Key and
    Workspace ID. Muxiva downloads and verifies the Agora macOS SDK, installs
    Qwen's Python WebSocket dependency, and installs the locked Pi TypeScript
    Agent packages for Demo 2.

## 0. Current support boundary

- The one-command path is verified on Apple Silicon macOS and pins Agora macOS SDK `4.6.2`.
- Demo 2 requires Node.js 22.19 or newer for the managed TypeScript Node Host and Pi.
- Qwen Nodes currently use the China (Beijing) endpoint. The API Key and
  Workspace ID must come from that same region.
- On Windows or another platform, download the SDK from the
  [official Agora SDK page](https://docs.agora.io/en/api-reference/sdks?product=voice)
  and pass its extracted directory to `setup.sh`.

## 1. Install Muxiva and the official Nodes

Install Git, Rust, Python 3, Node.js 22.19+, CMake 3.20+, and Xcode Command Line Tools, then run:

```bash
git clone https://github.com/PiyotaHu/muxiva.git
cd Muxiva
cargo install --locked --path crates/muxiva-cli
./examples/voice-agent/setup.sh
```

The final command:

1. downloads RTC Basic XCFrameworks from Agora's official CDN, using the
   [official macOS SDK repository](https://github.com/AgoraIO/AgoraRtcEngine_macOS/tree/4.6.2);
2. verifies every archive with SHA-256;
3. creates `examples/voice-agent/.muxiva/venv` and installs `websocket-client`;
4. checks out the pinned independent [Pi coding Agent](nodes/pi-agent.md),
   installs npm dependencies with lifecycle scripts disabled, and tests the
   adapter, Agent, and filesystem policy;
5. creates the Agent's default coding workspace;
6. builds the four C++ Nodes `agora.audio_source`, `agora.audio_sink`,
   `agora.data_source`, and `agora.data_sink`. They share one RTC Engine and Bot UID.

Installation is complete only after these lines appear:

```text
[MUXIVA][READY] Native, Python, and TypeScript Agent Node Packs are installed.
[MUXIVA][AGORA] sdk=.../build/vendor/agora-macos-4.6.2
[MUXIVA][QWEN]  python=.../.muxiva/venv/bin/python (no Qwen SDK download required)
[MUXIVA][AGENT] repository=https://github.com/PiyotaHu/muxiva-pi-agent.git ref=v0.2.1 commit=...
[MUXIVA][AGENT] workspace=.../.muxiva/workspaces/pi-agent permissions=list,read,search,create,replace,web-search
```

To use a manually downloaded SDK instead:

```bash
./examples/voice-agent/setup.sh /path/to/extracted-agora-sdk
```

## 2. Create an Agora App ID and tokens

For a first-time Agora account, follow the [field-by-field App ID, Certificate, and Token Builder guide](voice-credentials.md#a-create-an-agora-project-and-two-tokens).
The steps below are only a completion summary.

1. Sign up or log in to [Agora Console](https://console.agora.io/).
2. Open [Projects](https://console.agora.io/legacy/project-management), select
   **Create New**, and choose **Secured mode: APP ID + Token**.
3. Copy the project's **App ID**.
4. Choose one channel name, such as `muxiva-demo`. Every token below must use the
   exact same channel name.
5. Follow Agora's official [account and temporary-token guide](https://docs.agora.io/en/realtime-media/voice/manage-agora-account)
   or use the linked [Agora Token Builder](https://agora-token-generator-demo.vercel.app/)
   to create two short-lived RTC tokens:

| Studio field | UID | First-run role | Purpose |
| --- | ---: | --- | --- |
| Browser UID / Token | `1001` | Publisher | Browser microphone and playback |
| Muxiva Bot UID / Token | `2001` | Publisher | One C++ RTC Engine receives microphone and publishes assistant audio |

!!! warning "Never expose the App Certificate"
    The App Certificate belongs only on a token server. Never enter it in
    Studio, browser code, or Git. Temporary tokens are suitable for local
    evaluation; production deployments require a token server.

## 3. Create Qwen credentials

If Model Studio regions and workspaces are new to you, follow the [field-by-field API key and Workspace ID guide](voice-credentials.md#b-create-the-model-studio-key-and-workspace-id).
The key and Workspace ID must be a matching pair from the same China (Beijing) workspace.

1. Open [Alibaba Cloud Model Studio](https://bailian.console.aliyun.com/),
   select China (Beijing), and activate the service.
2. Follow the official [API Key guide](https://help.aliyun.com/en/model-studio/get-api-key)
   and save the plaintext when the Key is created.
3. Follow the official [first Qwen API call guide](https://help.aliyun.com/en/model-studio/first-api-call-to-qwen)
   to locate the **Workspace ID** in the same Workspace.

There is no Qwen SDK download step. Muxiva's Python Node talks directly to
the documented WebSocket/HTTP protocols. Realtime defaults to
`qwen-audio-3.0-realtime-flash`; the cascade uses Qwen ASR and TTS around a
[Pi coding Agent](nodes/pi-agent.md) backed by `qwen-flash`. It can really
read, search, create, and edit files inside a bounded workspace, and can use
the same Model Studio credentials for cited live web search. See the
[Agent integration SOP](nodes/agent-integration.md) for application-owned Agents.

## 4. Start Studio and enter the credentials

```bash
muxiva doctor --voice
./examples/voice-agent/run.sh
```

`doctor` should report both Agora packs as `mode=agora-native`, report
`qwen-python dependency=websocket`, and report
`pi-typescript-agent ... dependencies=locked`. With no credentials it prints every `MISSING` value;
that is a blocking diagnosis, not an optional hint. `run.sh` also prints `[MUXIVA][CLI]`; in a
source checkout it should point at this repository's `target/debug/muxiva` or
`target/release/muxiva`, preventing an older global CLI from serving stale Studio assets. In Studio:

1. Open **Connections**.
2. Enter the Model Studio API Key and Workspace ID.
3. Enter the Agora App ID, channel, and tokens for UIDs `1001` and `2001`.
4. Select **Save connections** and confirm both cards show **Ready**; otherwise the Runtime will not start.
5. Open **Templates** and choose **Qwen Realtime** for the first run.
6. Select **Run** in Studio and confirm the Runtime is live. Studio owns this management action.
7. Open **Voice Room**, select **Start live conversation**, and allow microphone access.
8. Confirm **MIC LEVEL** rises above `0%` while speaking, then speak again while the assistant is
   playing to verify full-duplex interruption.

Save connections writes values to `examples/voice-agent/.env` (mode `0600`, Git ignored).
Future runs load it automatically. You can also create it manually from `.env.example`.

After Realtime works, switch to **Pi Agent Full-Duplex Cascade (Demo 2)** to inspect
Qwen Server VAD + Streaming ASR → a stateful TypeScript Agent with Tool Calls →
cancellable Qwen TTS. Ask for the current time or today's weather to force a
real tool execution. Speak again during playback: Voice Room should enter interruption state,
old text and audio should stop, and the next transcript and answer should remain
in the same session. The session stays live until you select **End session**.

## Runtime logs and pipeline diagnosis

`run.sh` mirrors terminal output to `examples/voice-agent/.muxiva/runtime.log`. If both clients
look connected but there is no response, find the first signal that does not advance:

Open **◎ Observe** in Studio to correlate Nodes, Edges, and SDK-internal queues and highlight
yellow/red bottlenecks automatically. See [Observability and bottleneck diagnosis](observability.md)
for metric definitions, thresholds, and log filters.

1. Voice Room reports that the browser joined and published the microphone.
2. **MIC LEVEL** rises while speaking; after five seconds without speech energy the page identifies
   the input-device/permission problem directly.
3. The log reports `[MUXIVA][AGORA][participant.joined] uid=1001`.
4. The log reports `[MUXIVA][AGORA][audio.received]` and `agora-input` advances. In Observe, select
   `agora-audio-source`; `input.audio_peak_pcm16` must rise clearly above zero while speaking.
5. In Demo 1, the Qwen Realtime Node first logs Server VAD `speech_started` / `speech_stopped`,
   followed by ASR and `response.created`. In Demo 2, inspect
   `turn-to-agent` and `agent-to-response-gate`.
6. `qwen-audio` and `audio-to-room` advance and the browser plays the response.

The first missing signal identifies the failing layer. Credential values are never logged.

Voice Room renders each turn from Agora RTC data-stream messages—not the Studio NotificationBus—as chat
history: user ASR on the right and the Agent's streaming response on the left. Qwen incremental
ASR uses `text + stash` for the live preview and commits
the final text from `conversation.item.input_audio_transcription.completed`. The Agora Bot consumes
remote PCM without playing the user's voice on the Runtime machine and publishes assistant audio
as paced 10 ms PCM packets.

Closing Studio after the Runtime has been started does not change the RTC media/message protocol.
The bundled page currently uses `/api/v1/client/session` only to bootstrap temporary local browser
credentials. A production web app replaces that one endpoint with its token service and never
exposes Studio's Graph or Runtime management APIs.

## 5. Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `Agora SDK directory does not exist` | The path is not an extracted SDK; on macOS rerun `setup.sh` without arguments |
| `AgoraRtcKit.xcframework` not found | The manual package is for the wrong platform or incomplete; use the automatic installer |
| `qwen-python ready=false` | Run `setup.sh` again to recreate the project virtual environment |
| `pi-typescript-agent ready=false` | Install Node.js 22.19+ and rerun `setup.sh` to install the locked Pi packages |
| `installed Agora Node Packs are older` | Stop the currently running Studio, run `./examples/voice-agent/setup.sh` once, then start the demo again; `run.sh` rejects stale native code instead of silently loading it |
| Qwen authentication/model error | Key, Workspace ID, and model must belong to the same China (Beijing) Workspace |
| Agora cannot join | App ID, channel, and UID must exactly match token generation and the token must be unexpired |
| No microphone input | Allow microphone access for the local Studio page in browser site permissions |
| RTC Frames advance but nothing reacts | Check Voice Room **MIC LEVEL**, then Observe `input.audio_peak_pcm16`; values near zero mean silence or the wrong input device is being published |
| The top bar has no `◎ Observe` | Check `[MUXIVA][CLI]` in startup output; source development must use this checkout's `target/.../muxiva`, and rerunning `setup.sh` now builds and selects it |
| You clearly hear your own voice | Update Muxiva and rerun `setup.sh` to rebuild the Agora Node Pack; Bot logs must show `local_remote_playback=silenced-after-mix` |
| Text appears but no voice plays | Look for `[MUXIVA][AGORA][audio.published]`; if absent Qwen produced no audio, otherwise verify the browser subscribed to the Bot track |
| User ASR is missing | Look for Qwen `input_audio_transcription.completed`; the current page renders `text + stash` previews and final `transcript` |

## 6. Engineering verification

Without credentials, verify code, Node boundaries, and dynamic ABI loading:

```bash
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

These gates do not pretend to be a live call. Full acceptance means joining a
real Agora channel, using a real microphone, receiving Qwen text and audio, and
successfully interrupting playback.
