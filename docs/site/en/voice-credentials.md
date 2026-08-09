# Voice demo credentials: obtain and enter every value

This is the copy-and-follow path for a first run. You obtain **five values**: one Agora App ID,
two RTC tokens, one Model Studio API key, and one Workspace ID. Keep Muxiva's default channel and
two numeric UIDs.

!!! warning "Complete the project `.env` first"
    The headless Runtime does not depend on Studio. Save credentials in this checkout's
    `examples/voice-agent/.env`, then run `muxiva doctor --voice`. Studio Connections is only an
    optional graphical editor for the same file.

## Field map

| Service | Muxiva field | First-run value |
| --- | --- | --- |
| Agora | App ID | The 32-character App ID of your Agora project |
| Agora | Channel | `muxiva-demo` |
| Agora | Browser UID | `1001` |
| Agora | Browser Token | RTC token generated for channel `muxiva-demo`, UID `1001` |
| Agora | Muxiva Bot UID | `2001` |
| Agora | Muxiva Bot Token | RTC token generated for channel `muxiva-demo`, UID `2001` |
| Model Studio | API Key | Pay-as-you-go key created in China (Beijing) |
| Model Studio | Workspace ID | ID of the workspace that owns that key |

The Agora **App Certificate never goes into Muxiva**. It is used only by Token Builder.

## A. Create an Agora project and two tokens

### A1. Create the project

1. Sign in to [Agora Console](https://console.agora.io/).
2. Open [Projects](https://console.agora.io/legacy/project-management) and select **Create New**.
3. Enter a name and choose **Secured mode: APP ID + Token (Recommended)**.
4. Copy the **App ID** from the project list into `MUXIVA_AGORA_APP_ID` in `.env`.

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
| Channel name | `muxiva-demo` | `muxiva-demo` |
| Paste result into `.env` | `MUXIVA_AGORA_WEB_TOKEN` | `MUXIVA_AGORA_BOT_TOKEN` |

!!! important "You do not pre-create the channel"
    `muxiva-demo` is a case-sensitive room name agreed by every participant. It must match Token
    Builder and Studio character for character. UID-bound tokens are not interchangeable.

Agora Console also offers **Generate Temp Token**. For deterministic, separate browser and bot
identities, this guide uses Token Builder to generate two explicit numeric-UID tokens.

## B. Create the Model Studio key and Workspace ID

### B1. Activate the service and select the region

1. Sign in to [Alibaba Cloud Model Studio](https://bailian.console.aliyun.com/).
2. Complete activation or identity verification if prompted.
3. In the upper-right corner, select **China (Beijing)** and keep this region selected. Muxiva's
   current Qwen Nodes use the Beijing workspace endpoint.

### B2. Create the API key

1. Open **API Key** and select **Create API Key**.
2. For a first run, select the default workspace and all model permissions.
3. Copy the complete key immediately. If it is lost, reset it or create another key.
4. Paste it into `DASHSCOPE_API_KEY` in `.env`.

Official instructions: [obtain an API key](https://help.aliyun.com/en/model-studio/get-api-key).
Muxiva expects a pay-as-you-go Model Studio key, not a Coding Plan or Token Plan key.

### B3. Copy the Workspace ID that owns the key

1. Keep **China (Beijing)** selected.
2. Open the workspace control in the upper-right and copy the current **Workspace ID**, or copy it
   from Workspace Management.
3. Confirm that this is the same workspace selected when the API key was created.
4. Paste it into `DASHSCOPE_WORKSPACE_ID` in `.env`.

Official instructions: [obtain a Workspace ID](https://help.aliyun.com/en/model-studio/obtain-the-app-id-and-workspace-id).
A region or workspace mismatch causes WebSocket authentication failures.

There is no Qwen SDK download. Muxiva's Python Nodes use the documented WebSocket/HTTP
protocols, and `setup.sh` installs the Python dependency.

## C. Save the values once in Muxiva

```bash
cd /path/to/Muxiva
cp examples/voice-agent/.env.example examples/voice-agent/.env
# Edit the file with the values listed below.
# macOS defaults to Studio
./examples/voice-agent/run.sh
# Linux / Docker / servers use the Headless Runtime
./examples/voice-agent/run.sh --headless
```

1. Save the Agora and Model Studio fields in the project `.env`.
2. Run `muxiva doctor --voice` and resolve every `MISSING` line.
3. For macOS/Windows local development, run `run.sh` (or explicit `--studio`) and use Studio.
4. On Linux or in deployment, run `run.sh --headless` and wait for `runtime.started mode=headless`.
5. For headless mode, run `cd examples/voice-agent && npm run voice-room` in another terminal.
6. Open `http://127.0.0.1:4173`, test the Backend URL, and start the conversation.
7. Allow microphone access, say a complete sentence, and pause for the first response.

Credentials remain in the Git-ignored `examples/voice-agent/.env`; later runs load it automatically:

The file is local to the **current project checkout**. A fresh clone, another machine, or a second
working directory does not share it automatically. Explicitly copy the old project's `.env`, or
create it again from `.env.example`. When a value is absent, Muxiva now lists every missing field
and the absolute file path it read before it creates any Node Host.

```dotenv
MUXIVA_AGORA_APP_ID="..."
MUXIVA_AGORA_CHANNEL="muxiva-demo"
MUXIVA_AGORA_WEB_UID="1001"
MUXIVA_AGORA_WEB_TOKEN="..."
MUXIVA_AGORA_BOT_UID="2001"
MUXIVA_AGORA_BOT_TOKEN="..."
DASHSCOPE_API_KEY="..."
DASHSCOPE_WORKSPACE_ID="..."
```

## D. Verify and troubleshoot

```bash
muxiva doctor --voice
tail -f examples/voice-agent/.muxiva/runtime.log
```

`doctor` checks tooling, official Nodes, and credential presence. It does not issue tokens or print
secret values. Follow a real session in order:

1. Voice Room reports `Browser joined Agora` and `microphone published`.
2. The log reports `[MUXIVA][AGORA][participant.joined] uid=1001`.
3. The log reports `[MUXIVA][AGORA][audio.received]`.
4. The log reports `[MUXIVA][QWEN][event] type=input_audio_buffer.speech_started`.
5. `response.created`, `[MUXIVA][AGORA][data.published]`, and audio output begin increasing.
6. Voice Room diagnostics show increasing Client Messages and the chat renders both sides.

| Symptom | Check first |
| --- | --- |
| Agora cannot join | App ID, channel, UID, token binding, and token expiry |
| Only browser or bot joins | Browser UID `1001` and bot UID `2001` tokens may be swapped |
| Agora receives audio but Qwen has no events | Beijing region, key, and Workspace must match |
| Connected but no response | Finish a sentence and pause; inspect Qwen speech and response events |
| You hear your own voice | Close old Voice Room tabs, use headphones, and confirm AEC is enabled |

Next: [complete installation and run flow](voice-demo.md).
