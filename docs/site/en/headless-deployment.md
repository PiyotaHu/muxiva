# Headless Runtime and standalone web client

Muxiva has an explicit deployment boundary: **a Linux server runs the Graph; the user's device runs the web client**. Studio is an optional Graph design and local observability tool. It is neither the production Runtime nor the Voice Room web server.

```text
Linux / Docker                         macOS / Windows / phone browser
┌──────────────────────────┐           ┌──────────────────────────────┐
│ muxiva serve graph.json  │           │ Standalone Voice Room        │
│ Rust Runtime + Nodes     │◄─Agora───►│ microphone / speaker / chat  │
│ :8080 Bootstrap API only │◄──HTTPS───│ one-time RTC configuration   │
└──────────────────────────┘           └──────────────────────────────┘
```

Browser media and messages travel directly over Agora RTC. The HTTP API returns only the App ID, channel, web UID, and short-lived web token needed to join. It does not proxy media, edit the Graph, or expose the DashScope key, Agora bot token, or App Certificate.

## CLI contract

| Command | Purpose | Requires Studio |
| --- | --- | --- |
| `muxiva validate graph.json` | Compile and validate the Graph and Node Library | No |
| `muxiva run graph.json` | Execute a finite Graph; default deadline is 30 seconds | No |
| `muxiva serve graph.json` | Keep a real-time Graph alive and expose the minimal Bootstrap API | No |
| `muxiva studio graph.json` | Visual design, local debugging, and Observe | Studio itself |

Start the real voice Graph on Linux:

```bash
cd muxiva
./examples/voice-agent/setup.sh /absolute/path/to/agora-linux-sdk
cp examples/voice-agent/.env.example examples/voice-agent/.env
# Edit .env with Agora and Model Studio credentials.
muxiva doctor --voice
muxiva serve examples/voice-agent/graph.json
```

`./examples/voice-agent/run.sh --headless` is the portable convenience wrapper around
`muxiva serve`; Linux also selects this mode by default. macOS/Windows shells default to Studio,
so deployment automation should always include `--headless`. A successful start prints
`runtime.started mode=headless`, the Client API base URL, and `studio=not-required`. Ctrl-C and
container SIGTERM perform bounded shutdown. Logs remain in `.muxiva/runtime.log`.

## Start the standalone Voice Room

The static application lives in `examples/voice-agent/web/`, outside `.muxiva`, and Studio does not serve it. Its bundled Node script serves HTML/CSS/JS only—it does not run a Graph.

=== "macOS"

    ```bash
    cd examples/voice-agent
    npm run voice-room
    open http://127.0.0.1:4173
    ```

=== "Windows PowerShell"

    ```powershell
    cd examples\voice-agent
    npm run voice-room
    Start-Process http://127.0.0.1:4173
    ```

=== "Linux desktop"

    ```bash
    cd examples/voice-agent
    npm run voice-room
    xdg-open http://127.0.0.1:4173
    ```

Enter the **Backend URL** printed by `muxiva serve`, then select **Test connection** before starting a conversation. The local default is `http://127.0.0.1:8080`.

You may host the same static directory without Node:

```bash
docker run --rm -p 4173:80 \
  --mount type=bind,source="$PWD/examples/voice-agent/web",target=/usr/share/nginx/html,readonly \
  nginx:alpine
```

## Connect from a laptop to headless Linux

The recommended development path is an SSH tunnel. Keep `muxiva serve` on its default loopback bind, then run this on the macOS or Windows workstation:

```bash
ssh -L 8080:127.0.0.1:8080 user@linux-host
```

Start Voice Room locally and keep its Backend URL at `http://127.0.0.1:8080`. No public port or Client API token is required.

For a public address or Docker port mapping, save a separate 32+ character value as `MUXIVA_CLIENT_API_TOKEN` in the server project `.env`, then run:

```bash
muxiva serve examples/voice-agent/graph.json \
  --host 0.0.0.0 \
  --port 8080 \
  --allow-origin http://127.0.0.1:4173
```

Enter `http://SERVER_IP:8080` and that token in Voice Room. Muxiva refuses a non-loopback bind without a token and rejects wildcard CORS.

Production deployments must put both Voice Room and the Bootstrap API behind HTTPS and list the exact deployed web origin. Browsers normally deny microphone access to non-local HTTP pages, and an HTTPS page cannot call an HTTP API. Replace the development `.env` web token with an application Token Service that issues short-lived RTC credentials for authenticated users.

## HTTP surface

```text
GET /healthz                 # public readiness, no credentials
GET /api/v1/client/session   # browser RTC bootstrap; Bearer-protected remotely
OPTIONS ...                  # exact-origin CORS preflight
```

This is not the Studio API. It provides no Graph write, Node source, Run/Stop, Observe, or Connections mutation endpoints.
