# Stage 7: C ABI v1 and C++ Node SDK

Status: implemented pre-release vertical slice.

## Boundary

`muxiva-ffi` is the only workspace crate permitted to contain unsafe code. Its
checked-in normative header is `cpp/include/muxiva/muxiva.h`; the library and
header agree on `MUXIVA_ABI_VERSION_V1` (`0x00010000`). The crate builds as an
`rlib`, `staticlib`, and `cdylib`.

The ABI passes only fixed-width C scalars, POD views, callbacks, and 128-bit
value tokens. It never passes a Rust `Vec`, trait object, C++ container, smart
pointer, or virtual-object layout. `user_data` is opaque; the C++ trampolines
recover the private implementation only for the duration of a callback.

## Frames and ownership

`muxiva_frame_view_v1` contains the common header and a discriminated union for
audio, video, text, byte, signal, and event payloads. All views are borrowed
for one call. `muxiva_frame_copy_v1` validates version, exact v1 size, enum
values, zero reserved fields, UTF-8/identifier rules, null/length pairs, size
arithmetic, and the 16 MiB copy ceiling before storing owned bytes.

Copy is the only enabled v1 ownership mode. `MUXIVA_CAP_RETAIN_RELEASE` is
reserved but clear, and `muxiva_frame_retain_v1` returns `UNSUPPORTED`.
Foreign buffers may be reused immediately after a copy call returns.

## Handles and no-unwind rule

Runtime, session, frame, and node are distinct semantic handle kinds sharing
the POD `{slot, generation}` token layout. Registry slots are never exposed as
addresses. Slot reuse increments the generation; wrong-kind and stale tokens
return `INVALID_HANDLE`, while a repeated release of the current retired
generation returns the stable `CLOSED` failure. No release dereferences
caller-owned memory.

Every Rust export has a `catch_unwind` guard. All pointer access is isolated in
small helpers after null/alignment/length checks and has a local safety
comment. A caught Rust panic maps to `PANIC` without exposing panic payload
text. C++ lifecycle trampolines are `noexcept`, use `catch (...)`, and map an
exception to `FOREIGN_EXCEPTION`. `on_abort` is invoked once by `GraphRunner`
for a prepared foreign node after its callback fails.

Node release first closes new admission. An in-flight graph run makes release
return `BUSY`; destruction happens only after the run guard is gone. The
vtable's `destroy` callback executes once and never under the registry lock.

## Executable vertical slice

`muxiva_runtime_run_text_v1` is intentionally a focused Stage 7 bridge harness.
It constructs and runs the existing synchronous Rust graph:

```text
Rust text source -> C++ TransformNode -> Rust collecting sink
```

The C++ uppercase example executes `on_prepare`, `on_process`, and
`on_finish`; its throwing variant proves exception conversion and `on_abort`.
This is actual `GraphBuilder`/`GraphRunner` execution, not a direct C++ node
call masquerading as scheduling.

## Stage 8 boundary

Stage 6 does not yet publish a bounded external ingress/callback-drain API.
Stage 7 therefore does not expose an RTC adapter submission path and does not
invent an FFI-owned queue. Stage 8 must integrate an official core ingress
whose callback path only validates, copies, and non-blockingly submits; it
must also supply stopping admission, in-flight callback drain, and late
callback rejection. No C++ node user code may run on that SDK callback thread.

## Developer gates

`scripts/check-ffi.sh` builds the cdylib, compiles the public header as C11,
compiles the wrapper/example as C++17, links both to the same Cargo artifact,
and executes them. `scripts/check-ffi-asan.sh` adds AddressSanitizer and
UndefinedBehaviorSanitizer for the C++ side. On the Stage 7 macOS environment
both pass with Apple clang 21. CMake was unavailable and is not a build
prerequisite for this slice.

`clang-format` was absent. The small checked-in C/C++ surface was formatted
manually; adding a pinned formatter check is recorded debt.
