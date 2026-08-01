# Rust Nodes

Rust Nodes implement the Runtime's Node lifecycle directly.

```rust
use voxa_core::{Node, NodeContext};
use voxa_types::Frame;

pub struct MyNode;

impl Node for MyNode {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        Ok(())
    }
}
```

Built-in Rust Factories are compiled into the trusted Registry. Third-party
binary packages must cross Voxa's stable C ABI rather than depend on Rust's
unstable dynamic ABI.

## Current Studio boundary

Studio generates and stores Rust project source and its Manifest, but the
project build Host is not active yet. A production Host must generate a locked
crate, compile it, expose a stable ABI entrypoint, verify ABI and Manifest
identity, and report toolchain or compiler errors without loading invalid code.
