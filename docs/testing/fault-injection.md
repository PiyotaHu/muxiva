# Fault-injection matrix

| Boundary | Deterministic fault | Required invariant |
| --- | --- | --- |
| Frame | malformed layout, metadata, lineage | rejected before payload access |
| Graph/Node | lifecycle error or panic | one abort and reverse cleanup |
| Edge/Queue | full, closed, stalled consumer | bounded and policy-visible outcome |
| Managed stream | timeout, reconnect, late result | admission released; late output discarded |
| Signal/NotificationBus | slow or failing subscriber | publisher and other subscribers remain isolated |
| C/C++ ABI | short struct, stale handle, exception | no unwind; stable error; owned copy |
| Mock RTC | `mock_rtc_adapter_test`: ingress full, fixed loss/reorder window, held callback, concurrent/repeated leave | callback uses nonblocking ingress; copied payload survives caller mutation; context lives through eventual drain |
| Python domain | `test_muxiva.py`: exception, unsupported isolation, private loop, NotificationBus handoff | structured failure; Python executes only on its owned loop thread; publication only enqueues |
| Node domain | `domain.test.mjs`: throw, Promise return, full inbox, close then submit | structured failure; JS executes on its Worker; capacity remains bounded; late submission is rejected |
| CLI / Graph v1 | `cli_contract.rs`: existing init target and malformed graph passed to validate/run | no overwrite; validate and run use the same parser/compiler diagnostic |
| Studio HTTP | `muxiva-studio` unit contract: missing/forged bearer token and invalid authenticated graph | graph bytes never leak to unauthorized callers; validation uses Graph v1 compiler |
| Studio bind | `cli_contract.rs`: exact requested port held by a live listener | startup fails explicitly and never falls back to another port |

Every new adapter registers a scenario for malformed input, shutdown racing
with work, and ownership release before it can be described as supported.

The Stage 8 cases are exercised by `scripts/check-rtc.sh` and, with native
instrumentation, `scripts/check-rtc-asan.sh`. The Stage 9 cases are exercised
only after their packages have been built by `scripts/check-python.sh` and
`scripts/check-node.sh`; a source-only Rust workspace test is not evidence that
the importable Python wheel or Node native package works. Stage 10 authorization
and port cases use local sockets only and require no browser or network service.
