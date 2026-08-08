# Rust Node

Rust Node 直接实现 Runtime 的 Node 生命周期。

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
            // context.emit_signal(signal)?;       // 相邻图控制
            // context.publish_event(event)?;      // Runtime 全局 EventBus
        }
        Ok(()) // 回调状态，不承担消息传输
    }
}
```

Source Node 可调用 `context.schedule_next_tick(delay)` 保持活跃；不调用则完成
Source，从而兼容既有的一次性语义。

内置 Rust Factory 会编译进可信 Registry。第三方二进制 Package 必须跨越 Muxiva
稳定 C ABI，不能依赖不稳定的 Rust 动态 ABI。

## 当前 Studio 边界

Studio 可以生成并保存 Rust 项目源码与 Manifest，但项目 Build Host 尚未启用。
生产级 Host 必须生成锁定依赖的 Crate、执行编译、暴露稳定 ABI 入口、校验 ABI
与 Manifest 身份，并在不加载无效代码的前提下报告工具链或编译错误。
