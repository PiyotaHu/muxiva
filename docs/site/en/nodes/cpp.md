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
    // Any Node can call ctx.schedule_next_tick(std::chrono::milliseconds(20));
    // to request an internal callback without a clock Node in the Graph.
  }
  void on_signal(const muxiva_frame_view_v1& signal) override {
    // Receive graph control such as muxiva.voice.speech.started.
  }
};
```

The older `std::vector<GraphEmission>` return hook remains source-compatible,
but new Nodes should emit explicitly through the context. The V1 C ABI receives
Signals through `on_signal`; emitting new Signals or NotificationBus events from C++
still requires a future control-action context extension.

## Buffer ownership

`ctx.emit(port, frame)` is the safe default: it immediately copies every
borrowed header and payload, so a Node may reuse or destroy its SDK buffer as
soon as the call returns. Use it for control data, small payloads, and whenever
buffer ownership is uncertain.

High-rate Audio, Video, and Byte Sources may explicitly transfer a
`std::vector<uint8_t>` without copying:

```cpp
std::vector<std::uint8_t> pcm = receive_pcm();
auto frame = make_audio_view(pcm); // frame bytes point into pcm
ctx.emit_owned("audio_out", muxiva::OwnedFrame(frame, std::move(pcm)));
```

After `emit_owned`, the Node must not retain or mutate the payload. Muxiva keeps
the allocation alive across queues and Frame clones, then releases it after the
last clone is dropped; release may happen on any Runtime worker. The SDK falls
back to the safe-copy path when a new Node Pack runs on an older host, while old
Node Packs remain loadable by a new host. Header strings remain small copied
values. `emit_owned` intentionally accepts only Audio, Video, and Byte Frames.

The repository includes installable headers, CMake package configuration, and
independent consumer examples.

## Current Studio boundary

Studio generates and stores `node.cpp` and its Manifest, but project-package
compilation is not active yet. The planned Host must create CMake inputs,
compile a stable ABI library, verify the ABI and exact Factory identity, and
surface compiler diagnostics before the package becomes runnable.

Native implementations must test ownership, thread affinity, callbacks after
cancellation, buffer lifetime, errors, and bounded shutdown.
