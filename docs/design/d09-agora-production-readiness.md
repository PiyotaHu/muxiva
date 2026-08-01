# D09: Agora production readiness

## Goal

D09 turns the D07 adapter from a basic media bridge into an operable long-lived
provider. It covers token rotation, automatic reconnect visibility, bounded
control callbacks, quality telemetry, and a credential-driven live soak gate.
It does not claim live certification without a real Agora project and room.

## Lifecycle contract

`RtcAdapter` and `AgoraRtcClient` remain one-shot lifecycle owners. Agora owns
automatic reconnection inside the same engine. Every transition back to
`connected` after another state increments `connection_epoch`; a
`reconnecting -> connected` transition also increments `reconnects`. This lets
consumers separate observations from different connectivity periods without
recreating the provider.

`renew_token` is a hot operation. It validates a non-empty token, serializes
with C++ shutdown, invokes the vendor `renewToken` API, and records success or
failure. Neither the old nor new token is copied into Voxa events, statistics,
exceptions, or the soak summary. Applications obtain fresh tokens from their
own trusted server when `token_will_expire` or `token_required` is observed.

Shutdown closes admission before leaving and synchronously stops callbacks.
Late media remains contained and counted by the existing D07 drain contract.

## Callback isolation and observability

Vendor callbacks never execute user code. C++ converts them into bounded Voxa
Signal/Event frames. Python copies them into a bounded queue. Overflow is
observable and does not block an Agora callback thread.

The public snapshots include:

- current connection state, epoch, reconnects, and connection losses;
- token warning/request and renewal success/failure counts;
- network-quality samples and worst uplink/downlink ratings;
- latest call duration, transmitted/received bytes, user count, and last-mile
  delay;
- accepted/dropped control events and existing media queue counters.

Quality values remain Agora's integer ratings. Voxa does not reinterpret them
as percentages or invent service-level guarantees.

## Verification

Deterministic C++ and Python tests inject reconnect, token, quality, and call
statistics callbacks, exercise bounded queues, and prove tokens are absent from
events. The C++ provider is also compiled against real Agora API headers; the
Python wheel is probed with the real `agora-python-sdk==3.4.2.1` engine.

Live certification is explicit:

```sh
export VOXA_AGORA_APP_ID='...'
export VOXA_AGORA_CHANNEL='voxa-certification'
export VOXA_AGORA_TOKEN_FILE='/secure/runtime/token'
export VOXA_AGORA_SOAK_SECONDS=3600
export VOXA_AGORA_REQUIRE_AUDIO=1
VOXA_AGORA_PYTHON=.venv-agora/bin/python ./scripts/check-agora-live.sh
```

Replacing the token file atomically or updating its contents triggers a hot
renewal. The script prints only credential-free JSON metrics. It reports `SKIP`
when live credentials are absent, so ordinary public CI remains deterministic.

## Remaining certification debt

Before declaring a platform production-certified, run the live gate with two
clients on every supported OS/architecture, force a network interruption,
rotate a short-lived token, verify bidirectional media, and retain the summary
with the release evidence. Vendor binary packaging, device capture, compressed
codecs, and regional failover remain separate provider/release concerns.
