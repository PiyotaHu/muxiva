# C++ Nodes

The C++ SDK provides RAII wrappers over Muxiva's versioned C ABI. Multimodal
implementations emit named Frames through declared ports.

```cpp
#include <muxiva/muxiva.hpp>

class MyNode final : public muxiva::MultimodalGraphNode {
 public:
  void on_process(const muxiva_frame_view_v1* input,
                  muxiva::GraphNodeContext& ctx) override {
    // ctx.emit("text_out", output_frame);
    // A Source can call ctx.schedule_next_tick(std::chrono::milliseconds(20));
    // to schedule itself without a clock Node in the Graph.
  }
  void on_signal(const muxiva_frame_view_v1& signal) override {
    // Receive graph control such as muxiva.voice.speech.started.
  }
};
```

The older `std::vector<GraphEmission>` return hook remains source-compatible,
but new Nodes should emit explicitly through the context. The V1 C ABI receives
Signals through `on_signal`; emitting new Signals or EventBus events from C++
still requires a future control-action context extension.

The repository includes installable headers, CMake package configuration, and
independent consumer examples.

## Current Studio boundary

Studio generates and stores `node.cpp` and its Manifest, but project-package
compilation is not active yet. The planned Host must create CMake inputs,
compile a stable ABI library, verify the ABI and exact Factory identity, and
surface compiler diagnostics before the package becomes runnable.

Native implementations must test ownership, thread affinity, callbacks after
cancellation, buffer lifetime, errors, and bounded shutdown.
