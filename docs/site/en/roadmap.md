# Status and roadmap

Muxiva is pre-alpha. The foundation stages are implemented, while public package
distribution and several execution Hosts remain incomplete.

| Area | Status | Boundary |
| --- | --- | --- |
| Frames and concurrent Graph Runtime | Available | Static DAGs and exact typed ports |
| Backpressure and control plane | Available | Bounded queues, Signals, Events, turn control |
| C ABI and C++ SDK | Available | Versioned ABI and installable CMake package |
| Python and Node.js SDKs | Experimental | Managed execution domains and hosted text Factories |
| Studio | Available | Node Lab, typed wiring, validation, Run/Stop, metrics |
| Studio Python project Host | Experimental | Text Source, Transform, and Sink |
| Studio TypeScript project Host | Experimental | Node.js 22.19+, async lifecycle, bounded subprocess protocol |
| Studio Rust/C++ build Hosts | Planned | Source authoring exists; project compilation remains external |
| Agora and FFmpeg | Experimental | Mock and optional official Node paths |
| Package releases | Release-gated | CLI/Homebrew and Python automation are ready; publisher ownership is not fully confirmed |

## Near-term priorities

1. Harden and package the multi-language project Node execution Hosts.
2. Unify CLI, Runtime, and Studio project Registry and lockfile behavior.
3. Confirm Homebrew, PyPI, and npm publishers; ship the prepared CLI/Homebrew
   and Python channels; then add npm packages and C++ SDK archives.
4. Replace skipped Studio checks with real browser end-to-end coverage.
5. Add coverage, security audit, API/ABI compatibility, and release gates.
6. Retain live official-Node certification evidence for every release platform.

Breaking pre-alpha changes must be documented in the Changelog with migration
guidance.
