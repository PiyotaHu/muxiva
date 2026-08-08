# Stage 7 pre-release report

Stage 7 lands the first stable cross-language vertical slice.

## Delivered

- `muxiva-ffi` with `cdylib`, `staticlib`, and `rlib` artifacts and narrowly
  audited unsafe pointer operations.
- Checked-in C ABI v1 header and header-only C++17 RAII/node wrapper.
- Six frame payload POD variants, copy validation, bounded copied ownership,
  stable status/error output, and retain/release capability reservation.
- Generation-checked runtime/session/frame/node tokens with wrong-kind,
  stale-generation, repeated-release, close, and busy outcomes.
- No-unwind guards on Rust exports and exception-catching C++ trampolines.
- A C++ uppercase TransformNode and throwing-node case executed through an
  actual Rust source/transform/sink graph lifecycle.
- C11 header smoke, C++17 link/run integration, and ASan/UBSan developer
  scripts that work without CMake.

## Verification

The implementation passed:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
CC=clang CXX=clang++ ./scripts/check-ffi.sh
CXX=clang++ ./scripts/check-ffi-asan.sh
```

## Deferred and blockers

- Bounded external runtime ingress, adapter callback admission/drain, and RTC
  lifecycle are Stage 8 work because Stage 6 exposes no suitable public
  ingress primitive. This is the only material Stage 8 prerequisite.
- Retained foreign buffers remain unsupported; v1 is copy-only.
- The focused public run helper supports text transformation only. The POD ABI
  already represents all six frame types, but general graph authoring over C
  is deliberately not frozen in this stage.
- CMake and `clang-format` were unavailable in the implementation environment.
  Direct compiler integration and manual formatting were used; portable CMake
  packaging and pinned format automation remain developer-experience debt.
- No RTC, FFmpeg, Python, Node.js, Studio, dynamic loading, or real vendor SDK
  was added.
