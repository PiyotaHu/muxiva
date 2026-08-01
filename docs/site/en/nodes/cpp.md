# C++ Nodes

The C++ SDK provides RAII wrappers over Voxa's versioned C ABI. Multimodal
implementations emit named Frames through declared ports.

```cpp
#include <voxa/voxa.hpp>

class MyNode final : public voxa::MultimodalGraphNode {
 public:
  void on_process(const voxa_frame_view_v1* input,
                  voxa::GraphNodeContext& ctx) override {
    // ctx.emit("text_out", output_frame);
  }
};
```

The older `std::vector<GraphEmission>` return hook remains source-compatible,
but new Nodes should emit explicitly through the context. Signal and EventBus
actions require the planned control-action ABI extension and are not exposed by
the V1 C ABI yet.

The repository includes installable headers, CMake package configuration, and
independent consumer examples.

## Current Studio boundary

Studio generates and stores `node.cpp` and its Manifest, but project-package
compilation is not active yet. The planned Host must create CMake inputs,
compile a stable ABI library, verify the ABI and exact Factory identity, and
surface compiler diagnostics before the package becomes runnable.

Native implementations must test ownership, thread affinity, callbacks after
cancellation, buffer lifetime, errors, and bounded shutdown.
