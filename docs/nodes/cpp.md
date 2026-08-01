# C++ Node development

Use **Studio → Create Node → C++** to generate `node.cpp` and a Manifest.
Multimodal implementations derive from `voxa::MultimodalGraphNode` and emit
named Frames.

```cpp
#include <voxa/voxa.hpp>

class MyNode final : public voxa::MultimodalGraphNode {
 public:
  std::vector<voxa::GraphEmission> on_process(
      const voxa_frame_view_v1* input,
      std::string_view input_port) override {
    return {};
  }
};
```

Studio project-package compilation is **not active yet**. Saving registers the
package for authoring and discovery, but Studio keeps it out of runnable Graphs
until the planned C++ Host generates CMake inputs, compiles a stable C ABI
library, and verifies its ABI version. The standalone CMake development path is
available now in the [C++ SDK reference](../sdk/cpp.md).

## 中文

C++ Node 通过 CMake 构建并跨越稳定 C ABI。Studio 项目包编译 Host 尚未接通，
所以当前只保存和展示，不会允许加入可运行 Graph；独立 CMake 开发链路已经可用。
