# Voxa testing and quality gates

Voxa tests are layered so production code, native boundaries, language
bindings, the local Studio, and long-running safety tools remain independently
reproducible. `voxa-testkit` is workspace-internal and may only be used by
tests; production crates must never depend on it.

Run the ordinary offline-capable gate with:

```sh
./scripts/check-quality.sh
```

The gate always runs Rust, C/C++, and Mock RTC checks. Python and Node checks
are discovered after their Stage 9 scripts are present. Miri and fuzz scripts
print `SKIP` with a reason when their pinned tools are absent; a skip is never
reported as executed coverage.

Concurrency tests use named gates, explicit event scripts, or a manual clock.
Arbitrary sleeps are not accepted as synchronization. Timeout failures must
include the scenario and the last bounded state snapshot.

