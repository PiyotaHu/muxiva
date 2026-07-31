# @voxa/core

Voxa's Node-API package owns all frame bytes in Rust and executes each TypeScript
transform in a dedicated `worker_threads` event loop. Native producers notify
the worker through a bounded, non-blocking napi-rs `ThreadsafeFunction`.

V1 lifecycle callbacks are synchronous. Returning any Promise or thenable is a
structured `VOXA_NODE_PROMISE_UNSUPPORTED` failure. Queue-full and closed states
are returned locally; shutdown seals admission before terminating the worker,
and late output is ignored.

Use Node 22 LTS and run `pnpm install && pnpm check` in this directory.

