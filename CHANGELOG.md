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

### Changed

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

[Unreleased]: https://github.com/PiyotaHu/muxiva/compare/main...HEAD
