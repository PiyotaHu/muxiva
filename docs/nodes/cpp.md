# C++ Node development

Use **Studio → Create Node → C++** to generate `node.cpp` and a Manifest.
Multimodal implementations derive from `muxiva::MultimodalGraphNode` and emit
named Frames.

```cpp
#include <muxiva/muxiva.hpp>

class MyNode final : public muxiva::MultimodalGraphNode {
 public:
  void on_process(const muxiva_frame_view_v1* input,
                  muxiva::GraphNodeContext& ctx) override {
    // ctx.emit("text_out", output_frame);
  }
};
```

`ctx.emit` can be called zero, one, or many times. The previous vector-returning
override remains source-compatible, but new Nodes should use the context form.
The v1 C ABI transports data emissions; Signal and NotificationBus control actions are
planned as an additive ABI extension rather than being hidden in return values.

`ctx.emit(port, frame)` immediately owns a safe copy of all borrowed data. For
high-rate Audio, Video, or Byte Sources, move a `std::vector<uint8_t>` through
`ctx.emit_owned(port, muxiva::OwnedFrame(frame, std::move(payload)))`. The
Runtime then shares that allocation across queues and Frame clones and invokes
its release callback after the last clone is dropped. Do not access the payload
after transfer; release may run on any Runtime worker. New packs automatically
fall back to safe copy on older hosts, and older packs remain compatible with
new hosts.

Studio project-package compilation is **not active yet**. Saving registers the
package for authoring and discovery, but Studio keeps it out of runnable Graphs
until the planned C++ Host generates CMake inputs, compiles a stable C ABI
library, and verifies its ABI version. The standalone CMake development path is
available now in the [C++ SDK reference](../sdk/cpp.md).

## 中文

C++ Node 通过 CMake 构建并跨越稳定 C ABI。推荐实现接收
`GraphNodeContext&` 的生命周期函数，并用 `ctx.emit` 显式发送数据。v1 C ABI
的 `ctx.emit` 会立即深拷贝借用数据；高频 Audio、Video、Byte Source 可以用
`ctx.emit_owned(..., muxiva::OwnedFrame(frame, std::move(payload)))` 显式转移
Buffer 所有权并实现跨 FFI 零拷贝。转移后不得再访问 Payload，最终释放可能发生在
任意 Runtime Worker。ABI 会对旧 Host 自动回退到安全复制。Signal/NotificationBus
控制动作会通过后续的兼容扩展完成。Studio 项目包编译 Host 尚未接通，
所以当前只保存和展示，不会允许加入可运行 Graph；独立 CMake 开发链路已经可用。
