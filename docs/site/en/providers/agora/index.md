# Agora RTC transport

The official Agora transport Nodes are implemented in C++. Audio and data ingress/egress Nodes
for one RTC identity share one Engine, keep credentials outside Graph JSON, and exchange typed
PCM Audio Frames plus bounded, versioned client messages with Voxa. This version deliberately runs
one Agent RTC session per Runtime process; production scales with process/container isolation.

- Category: `transport`
- SDK: Agora RTC Native SDK `4.6.2`
- Implementation: `providers/transport/agora/cpp`
- Credentials: [field-by-field App ID and two-token guide](../../voice-credentials.md#a-create-an-agora-project-and-two-tokens)
- Setup: [real voice Agent guide](../../voice-demo.md)

Run on macOS:

```bash
./examples/voice-agent/setup.sh
voxa doctor --voice
```

Configure App ID, channel, Voxa Bot UID/token, and browser UID/token in Studio
**Connections**. Use short-lived tokens in development and a token server in production.

Do not generate only one token: the browser uses numeric UID `1001` and the Voxa Bot uses
`2001`, so generate two tokens for the same channel. The App Certificate never enters Voxa.

Nodes:

- [Agora Audio Ingress](audio-ingress.md)
- [Agora Audio Egress](audio-egress.md)
- [Realtime client messages](realtime-messages.md)
