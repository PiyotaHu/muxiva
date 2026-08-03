# Voice demo credentials: obtain and enter every value

This is the copy-and-follow path for a first run. You obtain **five values**: one Agora App ID,
two RTC tokens, one Model Studio API key, and one Workspace ID. Keep Voxa's default channel and
two numeric UIDs.

!!! warning "Do not select Run yet"
    Wait until both cards in Studio **Connections** show **Ready**, then select a Graph and open
    Voice Room.

## Field map

| Service | Voxa field | First-run value |
| --- | --- | --- |
| Agora | App ID | The 32-character App ID of your Agora project |
| Agora | Channel | `voxa-demo` |
| Agora | Browser UID | `1001` |
| Agora | Browser Token | RTC token generated for channel `voxa-demo`, UID `1001` |
| Agora | Voxa Bot UID | `2001` |
| Agora | Voxa Bot Token | RTC token generated for channel `voxa-demo`, UID `2001` |
| Model Studio | API Key | Pay-as-you-go key created in China (Beijing) |
| Model Studio | Workspace ID | ID of the workspace that owns that key |

The Agora **App Certificate never goes into Voxa**. It is used only by Token Builder.

## A. Create an Agora project and two tokens

### A1. Create the project

1. Sign in to [Agora Console](https://console.agora.io/).
2. Open [Projects](https://console.agora.io/legacy/project-management) and select **Create New**.
3. Enter a name and choose **Secured mode: APP ID + Token (Recommended)**.
4. Copy the **App ID** from the project list. This is Studio's App ID.

See Agora's official [account and project guide](https://docs.agora.io/en/realtime-media/voice/manage-agora-account).

### A2. Copy the App Certificate

1. Select the pencil icon for the project.
2. In Security, copy the **Primary Certificate**.
3. Keep it temporarily for Token Builder. Never put it in `.env`, a Graph, a web page, or Git.

### A3. Use Token Builder twice

Open [Agora Token Builder](https://agora-token-generator-demo.vercel.app/). Select **RTC** if a
product selector is shown. Both runs use the same App ID, Certificate, and channel; only the UID
changes.

| Token Builder field | First token | Second token |
| --- | --- | --- |
| App ID | Your App ID | The same App ID |
| App Certificate | Primary Certificate | The same Certificate |
| User ID / UID | `1001` | `2001` |
| Token expiration time | `3600` (one hour for evaluation) | `3600` |
| Channel name | `voxa-demo` | `voxa-demo` |
| Paste result into Studio | Browser Token | Voxa Bot Token |

!!! important "You do not pre-create the channel"
    `voxa-demo` is a case-sensitive room name agreed by every participant. It must match Token
    Builder and Studio character for character. UID-bound tokens are not interchangeable.

Agora Console also offers **Generate Temp Token**. For deterministic, separate browser and bot
identities, this guide uses Token Builder to generate two explicit numeric-UID tokens.

## B. Create the Model Studio key and Workspace ID

### B1. Activate the service and select the region

1. Sign in to [Alibaba Cloud Model Studio](https://bailian.console.aliyun.com/).
2. Complete activation or identity verification if prompted.
3. In the upper-right corner, select **China (Beijing)** and keep this region selected. Voxa's
   current Qwen Nodes use the Beijing workspace endpoint.

### B2. Create the API key

1. Open **API Key** and select **Create API Key**.
2. For a first run, select the default workspace and all model permissions.
3. Copy the complete key immediately. If it is lost, reset it or create another key.
4. Paste it into **Alibaba Cloud Model Studio → API Key** in Studio.

Official instructions: [obtain an API key](https://help.aliyun.com/en/model-studio/get-api-key).
Voxa expects a pay-as-you-go Model Studio key, not a Coding Plan or Token Plan key.

### B3. Copy the Workspace ID that owns the key

1. Keep **China (Beijing)** selected.
2. Open the workspace control in the upper-right and copy the current **Workspace ID**, or copy it
   from Workspace Management.
3. Confirm that this is the same workspace selected when the API key was created.
4. Paste it into Studio's **Workspace ID**.

Official instructions: [obtain a Workspace ID](https://help.aliyun.com/en/model-studio/obtain-the-app-id-and-workspace-id).
A region or workspace mismatch causes WebSocket authentication failures.

There is no Qwen SDK download. Voxa's Python Nodes use the documented WebSocket/HTTP
protocols, and `setup.sh` installs the Python dependency.

## C. Save the values once in Voxa

```bash
cd /path/to/Voxa
./examples/voice-agent/run.sh
```

1. Select **Connections** in Studio.
2. Fill both cards and select **Save connections**.
3. After both cards show **Ready**, select **Templates → Qwen Realtime**.
4. Select **Run** and wait until Studio reports that Runtime is active.
5. Open **Voice Room → Start live conversation** and allow microphone access.
6. Say a complete sentence and pause for about one second for the first response.

Studio saves credentials to `examples/voice-agent/.env` with mode `0600`; Git ignores the file
and later runs load it automatically:

```dotenv
VOXA_AGORA_APP_ID="..."
VOXA_AGORA_CHANNEL="voxa-demo"
VOXA_AGORA_WEB_UID="1001"
VOXA_AGORA_WEB_TOKEN="..."
VOXA_AGORA_BOT_UID="2001"
VOXA_AGORA_BOT_TOKEN="..."
DASHSCOPE_API_KEY="..."
DASHSCOPE_WORKSPACE_ID="..."
```

## D. Verify and troubleshoot

```bash
voxa doctor --voice
tail -f examples/voice-agent/.voxa/runtime.log
```

`doctor` checks tooling, official Nodes, and credential presence. It does not issue tokens or print
secret values. Follow a real session in order:

1. Voice Room reports `Browser joined Agora` and `microphone published`.
2. The log reports `[VOXA][AGORA][participant.joined] uid=1001`.
3. The log reports `[VOXA][AGORA][audio.received]`.
4. The log reports `[VOXA][QWEN][event] type=input_audio_buffer.speech_started`.
5. `response.created`, `[VOXA][AGORA][data.published]`, and audio output begin increasing.
6. Voice Room diagnostics show increasing Client Messages and the chat renders both sides.

| Symptom | Check first |
| --- | --- |
| Agora cannot join | App ID, channel, UID, token binding, and token expiry |
| Only browser or bot joins | Browser UID `1001` and bot UID `2001` tokens may be swapped |
| Agora receives audio but Qwen has no events | Beijing region, key, and Workspace must match |
| Connected but no response | Finish a sentence and pause; inspect Qwen speech and response events |
| You hear your own voice | Close old Voice Room tabs, use headphones, and confirm AEC is enabled |

Next: [complete installation and run flow](voice-demo.md).
