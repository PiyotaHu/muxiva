# Stage 11 pre-release note

Stage 11 introduces the workspace-internal `muxiva-testkit`, consolidated local
quality scripts, and documented deterministic fault boundaries. Optional Miri
and fuzz gates are honest capability checks: missing local toolchains are
reported as skips and are provisioned separately in CI.

The cross-stage gate now names the Stage 8 Mock RTC and Stage 9 built-package
scripts explicitly. Stage 10 adds loopback contract coverage for shared Graph
v1 diagnostics, create-only initialization, bearer-token rejection,
authenticated validation, and exact requested-port collisions. These tests use
real bound listeners with finite server joins rather than sleeps or guessed
free ports.

Performance baselines, cross-platform binding matrices, and real third-party
RTC/model SDK certification remain release engineering work and are not
claimed by this stage.
