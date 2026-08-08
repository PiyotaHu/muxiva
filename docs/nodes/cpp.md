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
The v1 C ABI transports data emissions; Signal and EventBus control actions are
planned as an additive ABI extension rather than being hidden in return values.

Studio project-package compilation is **not active yet**. Saving registers the
package for authoring and discovery, but Studio keeps it out of runnable Graphs
until the planned C++ Host generates CMake inputs, compiles a stable C ABI
library, and verifies its ABI version. The standalone CMake development path is
available now in the [C++ SDK reference](../sdk/cpp.md).

## 中文

C++ Node 通过 CMake 构建并跨越稳定 C ABI。推荐实现接收
`GraphNodeContext&` 的生命周期函数，并用 `ctx.emit` 显式发送数据。v1 C ABI
尚未承载 Signal/EventBus 控制动作，这会通过后续的兼容扩展完成。Studio 项目包编译 Host 尚未接通，
所以当前只保存和展示，不会允许加入可运行 Graph；独立 CMake 开发链路已经可用。
