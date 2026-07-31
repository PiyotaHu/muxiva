# Stage 8 pre-release report

## Delivered

- Rust-owned bounded external/session ingress with copy-only non-blocking
  submission, clone/close/release lifetime, consumption hook, and accounting.
- Versioned C RTC adapter ABI and C++ RAII owner.
- An in-memory mock SDK with its own callback thread and deterministic delay,
  loss, reorder, disconnect, late callback, and entry-barrier controls.
- Six callback categories, audio/video/text Copy ingress, stable lifecycle
  errors, atomic callback admission, in-flight drain timeout, and join-before-
  reclaim shutdown.
- C ABI, C++ integration, lifecycle/fault/race, ASan, and UBSan checks without
  CMake or a vendor SDK.

## Technical debt

- The mock has deterministic fields for disconnect and late events but the
  public test-only API only directly drives the callback-entry barrier.
- Control payload serialization is intentionally a minimal schema-versioned
  JSON object until the Stage 10 registry/schema layer supplies typed codecs.
- The external ingress consumer hook is intentionally focused. Attaching it to
  arbitrary live graph ports belongs to the general runtime/session API.
- ThreadSanitizer is not part of the default macOS script because its support
  depends on the installed clang/runtime. ASan and UBSan are mandatory here.
- Portable CMake packaging and pinned clang-format remain build-system debt.
