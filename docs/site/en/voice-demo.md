# Run the real voice agent from scratch

This guide starts from a clean macOS development environment and assumes no
prior Agora or Qwen knowledge. At the end, browser microphone audio travels
through Agora into a Voxa Graph, Qwen generates a live response, and Agora
plays it back.

!!! info "What you actually need"
    Agora requires an account, App ID, and three temporary RTC tokens. Qwen
    requires **no SDK download**—only an Alibaba Cloud Model Studio API Key and
    Workspace ID. Voxa downloads and verifies the Agora macOS SDK and installs
    the Qwen WebSocket dependency in an isolated Python environment.

## 0. Current support boundary

- The one-command path is verified on Apple Silicon macOS and pins Agora macOS SDK `4.6.2`.
- Qwen Providers currently use the China (Beijing) endpoint. The API Key and
  Workspace ID must come from that same region.
- On Windows or another platform, download the SDK from the
  [official Agora SDK page](https://docs.agora.io/en/api-reference/sdks?product=voice)
  and pass its extracted directory to `setup.sh`.

## 1. Install Voxa and the Providers

Install Git, Rust, Python 3, CMake 3.20+, and Xcode Command Line Tools, then run:

```bash
git clone https://github.com/PiyotaHu/Voxa.git
cd Voxa
cargo install --locked --path crates/voxa-cli
./examples/voice-agent/setup.sh
```

The final command:

1. downloads RTC Basic XCFrameworks from Agora's official CDN, using the
   [official macOS SDK repository](https://github.com/AgoraIO/AgoraRtcEngine_macOS/tree/4.6.2);
2. verifies every archive with SHA-256;
3. creates `examples/voice-agent/.voxa/venv` and installs `websocket-client`;
4. builds the `agora_audio_source` and `agora_audio_sink` C++ Node Packs.

Installation is complete only after these lines appear:

```text
[VOXA][READY] Native and Python Node Packs are installed.
[VOXA][AGORA] sdk=.../build/vendor/agora-macos-4.6.2
[VOXA][QWEN]  python=.../.voxa/venv/bin/python (no Qwen SDK download required)
```

To use a manually downloaded SDK instead:

```bash
./examples/voice-agent/setup.sh /path/to/extracted-agora-sdk
```

## 2. Create an Agora App ID and tokens

1. Sign up or log in to [Agora Console](https://console.agora.io/).
2. Open [Projects](https://console.agora.io/legacy/project-management), select
   **Create New**, and choose **Secured mode: APP ID + Token**.
3. Copy the project's **App ID**.
4. Choose one channel name, such as `voxa-demo`. Every token below must use the
   exact same channel name.
5. Follow Agora's official [account and temporary-token guide](https://docs.agora.io/en/realtime-media/voice/manage-agora-account)
   or use the linked [Agora Token Builder](https://agora-token-generator-demo.vercel.app/)
   to create three short-lived RTC tokens:

| Studio field | UID | First-run role | Purpose |
| --- | ---: | --- | --- |
| Browser UID / Token | `1001` | Publisher | Browser microphone and playback |
| Ingress Bot UID / Token | `2001` | Publisher | C++ Node receives browser audio |
| Egress Bot UID / Token | `2002` | Publisher | C++ Node publishes assistant audio |

!!! warning "Never expose the App Certificate"
    The App Certificate belongs only on a token server. Never enter it in
    Studio, browser code, or Git. Temporary tokens are suitable for local
    evaluation; production deployments require a token server.

## 3. Create Qwen credentials

1. Open [Alibaba Cloud Model Studio](https://bailian.console.aliyun.com/),
   select China (Beijing), and activate the service.
2. Follow the official [API Key guide](https://help.aliyun.com/en/model-studio/get-api-key)
   and save the plaintext when the Key is created.
3. Follow the official [first Qwen API call guide](https://help.aliyun.com/en/model-studio/first-api-call-to-qwen)
   to locate the **Workspace ID** in the same Workspace.

There is no Qwen SDK download step. Voxa's Python Provider talks directly to
the documented WebSocket/HTTP protocols; `setup.sh` installs its only external
Python dependency. Realtime defaults to `qwen-audio-3.0-realtime-flash`; the
cascade uses Qwen ASR, LLM, and TTS.

## 4. Start Studio and enter the credentials

```bash
voxa doctor --voice
./examples/voice-agent/run.sh
```

`doctor` should report both Agora packs as `mode=agora-native` and report
`qwen-python dependency=websocket`. In Studio:

1. Open **Connections**.
2. Enter the Model Studio API Key and Workspace ID.
3. Enter the Agora App ID, channel, and tokens for UIDs `1001`, `2001`, `2002`.
4. Open **Templates** and choose **Qwen Realtime** for the first run.
5. Open **Voice Room**, select **Start live conversation**, and allow microphone access.
6. Speak naturally, then speak again while the assistant is playing to verify full-duplex interruption.

After Realtime works, switch to **Qwen Cascade** to inspect VAD → ASR → LLM →
TTS. The session remains live until you select **End session**.

## 5. Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `Agora SDK directory does not exist` | The path is not an extracted SDK; on macOS rerun `setup.sh` without arguments |
| `AgoraRtcKit.xcframework` not found | The manual package is for the wrong platform or incomplete; use the automatic installer |
| `qwen-python ready=false` | Run `setup.sh` again to recreate the project virtual environment |
| Qwen authentication/model error | Key, Workspace ID, and model must belong to the same China (Beijing) Workspace |
| Agora cannot join | App ID, channel, and UID must exactly match token generation and the token must be unexpired |
| No microphone input | Allow microphone access for the local Studio page in browser site permissions |

## 6. Engineering verification

Without credentials, verify code, Provider boundaries, and dynamic ABI loading:

```bash
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

These gates do not pretend to be a live call. Full acceptance means joining a
real Agora channel, using a real microphone, receiving Qwen text and audio, and
successfully interrupting playback.
