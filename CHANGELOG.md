# Changelog

All notable changes to Muxiva will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and releases will follow [Semantic Versioning](https://semver.org/) once public
package contracts are enabled. During pre-alpha, breaking changes are allowed
but must be called out explicitly with migration guidance.

## [Unreleased]

### Added

- Strictly separated English and Simplified Chinese documentation sources with
  contextual language switching and translation-parity CI validation.
- Project Node Library and `muxiva.node/v1` Manifest authoring in Studio.
- Trusted local text Python Node execution Host.
- Palette drag-to-canvas and typed port-to-port Edge wiring.
- MkDocs documentation site deployed through GitHub Pages.
- Managed Studio TypeScript Project Node Host with asynchronous lifecycle,
  exact module entrypoints, bounded framed output, cancellation, and structured
  subprocess diagnostics.
- Vendor-neutral `@muxiva/agent` TypeScript contract for stateful streaming
  Agent Nodes, plus a project-local Pi Agent integration in Demo 2 with Qwen,
  Tool Calls, session state, and full-duplex cancellation.
- Studio **Observe** dashboard with per-Node callback latency and custom
  counters/gauges, per-Edge queue age/occupancy/throughput, automatic hotspot
  verdicts, click-through diagnosis, structured runtime summaries, bounded
  cross-session trend history, authenticated Prometheus scraping, and
  non-blocking OTLP/HTTP JSON metric export.

### Changed

- Demo 2 now composes Qwen VAD/ASR, a tool-using Pi TypeScript Agent, and Qwen
  TTS instead of using the single-purpose Python Qwen LLM Node.

- **Breaking:** renamed the process-local observability `EventBus` to
  `NotificationBus` so it cannot be confused with Graph `EventFrame` output
  ports. Rust now uses `NotificationBus`, `with_notification_bus`,
  `notification_bus`, and `publish_notification`; Python uses
  `muxiva.NotificationBus` and `ctx.publish_notification`; TypeScript uses
  `NotificationBus` and `ctx.publishNotification`. Graph `event_out` ports and
  `EventFrame` remain unchanged.
- **Breaking:** unified the project under the Muxiva name across the `muxiva` CLI,
  Rust crates, `muxiva` Python package, `@muxiva/core` TypeScript package,
  `Muxiva::` CMake targets, `muxiva_*` C ABI, `.muxiva` project metadata,
  `MUXIVA_*` environment variables, logs, documentation, and repository URLs.
- Upgraded the Python binding to PyO3 0.29 and the Node binding to NAPI-RS 3,
  including the new typed ThreadsafeFunction API and NAPI CLI configuration.
- Corrected the scheduled Fuzz and Miri workflows to provision their required
  pinned nightly Rust toolchains.
- Dependabot now keeps coupled NAPI crates together and avoids grouping
  unrelated Node.js major-version migrations.
- README and Node development guides now point to the public documentation
  site and describe current language Host boundaries explicitly.

### Security

- Updated PyO3 to 0.29.0, resolving the published iterator out-of-bounds read,
  missing `Sync` bound, and `PyString::from_object` buffer-safety advisories.

### Fixed

- Node Worker shutdown now waits for the native execution domain to acknowledge
  closure before terminating the Worker, preventing environment teardown races.
- Agora audio ingress now drains 10 ms SDK packets at real-time cadence with a
  bounded catch-up burst instead of forwarding only one packet every 20 ms,
  which previously created multi-second ASR and TTS latency.

[Unreleased]: https://github.com/PiyotaHu/muxiva/compare/main...HEAD
