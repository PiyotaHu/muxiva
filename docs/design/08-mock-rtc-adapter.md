# Stage 8: C++ RTC adapter contract and mock RTC

Status: implemented pre-release contract.

## Boundary and ABI

`rtc_adapter_v1.h` is a C-compatible, versioned companion to the Stage 7
`muxiva.h`. Every extensible structure begins with `abi_version` and
`struct_size`; all packets are borrowed for the dynamic call only. The raw C
handle has single-owner destroy semantics, while `muxiva::MockRtc` makes reset
and destruction idempotent for C++ callers. Every exported C++ entry catches
all exceptions.

The adapter exposes create, connect/join, audio/video/text send, leave,
statistics, and destroy. Sends are legal only in Connected and make their
owned copy before returning. V1 deliberately has no zero-copy SDK mode.

## Rust-owned external ingress

Stage 8 adds the narrow `muxiva_session_ingress_v1` contract to `muxiva-ffi`.
Ingress handles are generation-checked, cloneable heap-owned references. They
have fixed item and byte budgets, non-blocking `try_submit`, explicit close,
stats, and a focused `try_pop` consumer hook. A busy queue mutex is treated as
full rather than allowing an SDK callback to wait. Submission validates and
copies through the Stage 7 six-frame contract and never invokes a graph node.

## Threads, faults, and shutdown

`InMemoryMockRtc` owns one callback worker and a scheduled event deque. Delay,
drop-every-N, reorder window, injected disconnect, allowed late work, and a
callback-entry barrier are deterministic configuration fields. Sequence IDs
make outcomes observable.

Callbacks acquire a shared callback context, increment `in_flight`, and install
an unconditional decrement/notifier guard. Shutdown publishes
`accepting=false`, closes ingress, rejects production, then waits to a
monotonic drain deadline. A late callback reads only atomic/context state and
does not touch the adapter or callback user data. Timeout never frees the
context: final destroy releases the barrier, joins the callback thread outside
locks, and only then reclaims it and its retained ingress handle.

Media is submitted only through external ingress. Connection, participant,
and interruption notifications additionally map to namespaced Signal frames;
errors and custom notifications map to namespaced Event frames. All callback
views are ephemeral and all mapped payloads carry `schema_version`.

## Verification

`check-rtc.sh` compiles the C ABI smoke and C++17 integration directly with
Apple clang. `check-rtc-asan.sh` adds AddressSanitizer and UndefinedBehavior
Sanitizer. Tests cover copy ownership, callback thread identity, queue-full,
validation, deterministic loss/reorder, repeated leave/reset, send-after-leave,
held-callback drain timeout, eventual drain, and safe late-drop accounting.
