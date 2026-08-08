# Rust Nodes

Rust Nodes implement the Runtime's Node lifecycle directly.

```rust
use muxiva_core::{Node, NodeContext};
use muxiva_core::PortName;
use muxiva_types::Frame;

pub struct MyNode;

impl Node for MyNode {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        if let Some(frame) = input {
            context.emit(PortName::new("text_out").expect("valid Port"), frame)?;
            // context.emit_signal(signal)?;       // adjacent graph control
            // context.publish_event(event)?;      // runtime-wide EventBus
        }
        Ok(()) // callback status, not the message transport
    }
}
```

Source Nodes may call `context.schedule_next_tick(delay)` to remain active.
Omitting it completes the source, preserving one-shot behavior.

Built-in Rust Factories are compiled into the trusted Registry. Third-party
binary packages must cross Muxiva's stable C ABI rather than depend on Rust's
unstable dynamic ABI.

## Current Studio boundary

Studio generates and stores Rust project source and its Manifest, but the
project build Host is not active yet. A production Host must generate a locked
crate, compile it, expose a stable ABI entrypoint, verify ABI and Manifest
identity, and report toolchain or compiler errors without loading invalid code.
