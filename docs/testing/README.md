# Voxa testing and quality gates

Voxa tests are layered so production code, native boundaries, language
bindings, the local Studio, and long-running safety tools remain independently
reproducible. `voxa-testkit` is workspace-internal and may only be used by
tests; production crates must never depend on it.

Run the ordinary offline-capable gate with:

```sh
./scripts/check-quality.sh
```

The gate runs Rust, C/C++, Mock RTC, Python, and Node checks when their checked-in
stage scripts and local toolchains are available. It does not contact a real RTC
service, open a browser, or call a model service. Miri and fuzz scripts print
`SKIP` with a reason when their pinned tools are absent; a skip is never reported
as executed coverage.

## Stage 8–10 integration gates

| Surface | Command | Deterministic coverage | Local prerequisite |
| --- | --- | --- | --- |
| Stage 8 Mock RTC | `./scripts/check-rtc.sh` | C ABI smoke; copied audio/video/text; bounded ingress; scripted loss/reorder; callback-thread ownership; repeated/concurrent leave; held-callback drain | Cargo, C11/C++17 compiler; current script uses `xcrun` and is therefore a macOS gate |
| Stage 8 native safety | `./scripts/check-rtc-asan.sh` | The same adapter contract under AddressSanitizer and UndefinedBehaviorSanitizer | clang with ASan/UBSan plus the macOS SDK |
| Stage 9 Python | `VOXA_PYTHON=/path/to/python ./scripts/check-python.sh` | Built wheel, immutable frames, private event-loop thread, structured exceptions, bounded domain/event delivery | supported Python with `maturin` and `pytest` already installed |
| Stage 9 Node | `VOXA_NODE_HOME=/path/to/node ./scripts/check-node.sh` | Native package build, dedicated Worker execution, throws, rejected Promise returns, bounded admission, close behavior | Node 20–24 and `pnpm`; dependency store must already be populated for offline use |
| Stage 9 combined | `VOXA_PYTHON=/path/to/python ./scripts/check-stage9-sanitizers.sh` | workspace Rust tests followed by both built binding packages | all Python and Node prerequisites above |
| Stage 10 CLI/Studio | `cargo test --offline -p voxa-studio -p voxa-cli` | shared Graph v1 diagnostics, create-only init, bearer-token rejection, authenticated validation, and exact occupied-port failure | loopback sockets must be permitted by the test sandbox |
| D07 Agora C++ | `./scripts/check-rtc.sh` and `./scripts/check-rtc-asan.sh` | fake-SDK PCM16/I420 copy, bounded ingress, signals/events, outbound media, idempotent close, deliberately late callback | C++17 compiler; vendor SDK is not required |
| D07 Agora Python | `VOXA_AGORA_PYTHON=/path/to/python3.9 ./scripts/check-agora-python.sh` | imports the real community extension and checks its engine/audio/video observer surface | CPython 3.9 with `agora-python-sdk==3.4.2.1`; otherwise explicit `SKIP` |
| D08 media | `./scripts/check-media.sh` | exact PCM/video shapes, rate conversion, I420/RGBA conversion, timestamp continuity/discontinuity, byte budget, concurrent admission, flush/reset | C++17 compiler; real FFmpeg test runs when development libraries are discoverable |
| D08 media safety | `./scripts/check-media-asan.sh` | provider-independent contract and, when discoverable, the real FFmpeg backend under ASan/UBSan | clang with ASan/UBSan |

The Linux `check-native-tsan.sh` gate also executes the D08 concurrent-admission
contract under ThreadSanitizer.

The Stage 8 delay setting models an adapter fault. Test completion is observed
through adapter state/counters and a bounded deadline; the delay is not used to
guess that a callback ran. Stage 10 socket tests bind port `0` or hold an actual
loopback listener, so they never select a port by probing and then racing another
process to bind it.

Stage 9 package scripts build the importable artifacts before running tests.
They are not dependency bootstrap scripts: prepare the Python tools and Node
lockfile store separately. `check-node.sh` invokes `pnpm install
--frozen-lockfile`, which may access the registry when its store is cold.

Concurrency tests use named gates, explicit event scripts, or a manual clock.
Arbitrary sleeps are not accepted as synchronization. Timeout failures must
include the scenario and the last bounded state snapshot.
