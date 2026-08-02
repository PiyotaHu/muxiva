# Agora RTC transport

Agora is a C++ transport Provider. It joins a channel with separate ingress and egress clients,
keeps credentials outside Graph JSON, and exchanges typed PCM Audio Frames with Voxa.

- Category: `transport`
- SDK: Agora RTC Native SDK `4.6.2`
- Implementation: `providers/transport/agora/cpp`
- Setup: [real voice Agent guide](../../voice-demo.md)

Run on macOS:

```bash
./examples/voice-agent/setup.sh
voxa doctor --voice
```

Configure App ID, channel, ingress UID/token, egress UID/token, and browser UID/token in Studio
**Connections**. Use short-lived tokens in development and a token server in production.

Nodes:

- [Agora Audio Ingress](audio-ingress.md)
- [Agora Audio Egress](audio-egress.md)
