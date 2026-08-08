# Rust Node development

Use **Studio → Create Node → Rust** to generate a package skeleton. Rust Nodes
implement `muxiva_core::Node`; third-party binary packages must cross Muxiva's
stable C ABI rather than relying on Rust's unstable dynamic ABI.

```rust
use muxiva_core::{Node, NodeContext};
use muxiva_types::Frame;

pub struct MyNode;

impl Node for MyNode {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        // Validate the input and emit through a declared output Port.
        Ok(())
    }
}
```

Studio project-package compilation is **not active yet**. It saves the package
for authoring and discovery but keeps it out of runnable Graphs until the
planned Rust Host can generate a crate, compile it across the stable C ABI, and
report missing toolchains or compiler diagnostics. Core concepts are documented
in the [Node/Graph design](../design/04-node-graph-and-sync-runner.md).

## 中文

Rust Node 实现统一的 `Node` 生命周期。Studio 项目包构建 Host 尚未接通，当前
只保存和展示；后续必须通过稳定 C ABI 加载并明确报告工具链或编译错误。
