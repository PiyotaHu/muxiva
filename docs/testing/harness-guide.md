# Harness guide

Use `TestGate` to hold a worker at a named state and let the controller observe
arrival before releasing it. Use `ManualClock` for flow/deadline state changes,
`ThreadProbe` for execution-domain assertions, and `LeakProbe` for test-owned
create/destroy accounting. Logs and payload previews must stay bounded.

Tests should assert a finite event sequence and explicit counters. A timeout is
only a liveness ceiling and must not be the mechanism that orders the test.

