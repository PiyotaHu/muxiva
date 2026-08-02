# Voice demo credentials, field by field

If you only have an Agora App ID, setup is **1/7 complete**. Do not select **Run** or open
**Voice Room** yet. Obtain the other six values below and wait until both cards in Connections
show **Ready**.

!!! warning "Never post secrets in an issue, chat, or Git"
    An App ID is not a secret, but the App Certificate, RTC tokens, and Qwen API key are.
    Temporary tokens are for local evaluation. Production requires a server-side token service.

## Every field you will fill

Start Studio with `./run.sh`, then select **Connections** in the top toolbar.

### Agora RTC

| Studio field | First-run value | Where it comes from |
| --- | --- | --- |
| App ID | Your 32-character Agora App ID | Projects in Agora Console |
| Channel | Keep `voxa-demo` | A channel name you choose |
| Browser UID | Keep `1001` | Voxa default |
| Browser Token | RTC token for App ID + `voxa-demo` + UID `1001` | Agora Token Builder |
| Ingress Bot UID | Keep `2001` | Voxa default |
| Ingress Bot Token | RTC token for App ID + `voxa-demo` + UID `2001` | Agora Token Builder |
| Egress Bot UID | Keep `2002` | Voxa default |
| Egress Bot Token | RTC token for App ID + `voxa-demo` + UID `2002` | Agora Token Builder |

### Alibaba Cloud Model Studio

| Studio field | Value | Where it comes from |
| --- | --- | --- |
| API Key | Model Studio API key created in China (Beijing) | API Key page in Model Studio |
| Workspace ID | ID of the workspace that owns that key | Workspace menu in the top-right corner |

## Step 1: generate three Agora tokens

### Find the App Certificate

1. Open [Agora Console](https://console.agora.io/).
2. Open **Projects** and find the project that owns the App ID.
3. Select its edit icon and copy **Primary Certificate** from Security.

The App Certificate is used only to create tokens. **Never enter it in Studio.**

### Use Token Builder three times

Open [Agora Token Builder](https://agora-token-generator-demo.vercel.app/) and select RTC.
Use the same App ID, App Certificate, and `voxa-demo` Channel every time:

| Generation | Numeric UID | Paste the token into |
| ---: | ---: | --- |
| 1 | `1001` | Browser Token |
| 2 | `2001` | Ingress Bot Token |
| 3 | `2002` | Egress Bot Token |

For the first evaluation, Publisher permission and a short expiry long enough for the test are
the simplest choices. Use **numeric UIDs**. Tokens are not interchangeable. Agora documents
temporary-token generation in its [official account and token guide](https://docs.agora.io/en/realtime-media/voice/manage-agora-account).

## Step 2: obtain the two Qwen values

1. Open [Alibaba Cloud Model Studio](https://bailian.console.aliyun.com/) and select China
   (Beijing) in the top-right corner.
2. Open API Key, create a pay-as-you-go key, and copy it immediately.
3. Open the top-right workspace menu and copy the **Workspace ID** that owns the key.
4. Ensure both values belong to the same region and workspace.

Official guides: [obtain an API key](https://help.aliyun.com/en/model-studio/get-api-key) ·
[obtain a Workspace ID](https://help.aliyun.com/en/model-studio/obtain-the-app-id-and-workspace-id).

## Step 3: fill, confirm, and run

```bash
cd examples/voice-agent
./run.sh
```

1. Select **Connections** in Studio.
2. Fill the tables above and select **Save connections**.
3. Confirm that both Agora RTC and Alibaba Cloud Model Studio show **Ready**.
4. Select **Templates**, then use **Qwen Realtime** for the first run.
5. Open **Voice Room** and select **Start live conversation**.
6. Allow microphone access and begin speaking.

Connections currently use Studio process memory. Closing Studio requires entering the values
again. Tokens and API keys are never written to the Graph or Git.

## What `doctor --voice` actually checks

`voxa doctor --voice` diagnoses the environment; it does not issue credentials:

- `native-node-pack PASS` means the Agora C++ Node Packs compiled correctly.
- `qwen-python PASS` means the Python WebSocket dependency works.
- `voice-credentials WARN/MISSING` lists values absent from the current shell environment.
- `--strict` returns a non-zero status for any missing prerequisite and is suitable for CI.

Values entered in Studio live in another process, so a separate doctor command cannot see them.
Studio performs another preflight for the active Graph. Missing values open Connections and do
not allow a C++ exception to terminate the process.

Next: [the complete install and run flow](voice-demo.md).
