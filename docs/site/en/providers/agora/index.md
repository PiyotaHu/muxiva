# Agora RTC transport

Agora is a C++ transport Provider. Ingress and egress Nodes share one process-level RTC Engine,
keeps credentials outside Graph JSON, and exchanges typed PCM Audio Frames with Voxa.

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
