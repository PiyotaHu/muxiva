# Rust Node

Rust Node 直接实现 Runtime 的 Node 生命周期。

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

内置 Rust Factory 会编译进可信 Registry。第三方二进制 Package 必须跨越 Voxa
稳定 C ABI，不能依赖不稳定的 Rust 动态 ABI。

## 当前 Studio 边界

Studio 可以生成并保存 Rust 项目源码与 Manifest，但项目 Build Host 尚未启用。
生产级 Host 必须生成锁定依赖的 Crate、执行编译、暴露稳定 ABI 入口、校验 ABI
与 Manifest 身份，并在不加载无效代码的前提下报告工具链或编译错误。
