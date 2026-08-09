# C++ Node

C++ SDK 在 Muxiva 版本化 C ABI 之上提供 RAII Wrapper。多模态实现通过声明的
Port 输出具名 Frame。

```cpp
#include <muxiva/muxiva.hpp>

class MyNode final : public muxiva::MultimodalGraphNode {
 public:
  void on_process(const muxiva_frame_view_v1* input,
                  muxiva::GraphNodeContext& ctx) override {
    // ctx.emit("text_out", output_frame);
    // 任意 Node 可调用 ctx.schedule_next_tick(std::chrono::milliseconds(20));
    // 安排下一次内部回调，不需要在 Graph 里连接时钟 Node。
  }
  void on_signal(const muxiva_frame_view_v1& signal) override {
    // 接收 muxiva.voice.speech.started 等图内控制信号。
  }
};
```

旧的 `std::vector<GraphEmission>` 返回 Hook 继续保持源码兼容，新 Node 应通过
Context 显式发送。V1 C ABI 已支持通过 `on_signal` 接收 Signal；从 C++ 主动发送
Signal 或 NotificationBus 事件仍需要后续控制动作 Context 扩展。

## Buffer 所有权

`ctx.emit(port, frame)` 是安全默认接口：它会立即复制所有借用的 Header 与
Payload，因此调用返回后，Node 可以立刻复用或销毁 SDK Buffer。控制数据、小
Payload 或所有权不明确时都应该使用它。

高频 Audio、Video、Byte Source 可以显式转移 `std::vector<uint8_t>` 的所有权，
避免复制大块媒体数据：

```cpp
std::vector<std::uint8_t> pcm = receive_pcm();
auto frame = make_audio_view(pcm); // frame 的 bytes 指向 pcm
ctx.emit_owned("audio_out", muxiva::OwnedFrame(frame, std::move(pcm)));
```

调用 `emit_owned` 后，Node 不得继续持有或修改 Payload。Muxiva 会让同一块内存
安全穿过队列和 Frame Clone，并在最后一个 Clone 销毁后释放；释放可能发生在任意
Runtime Worker 线程。新 Node Pack 运行在旧 Host 时，SDK 会自动退回安全复制；
旧 Node Pack 也仍可被新 Host 加载。Header 字符串仍是小对象复制。
`emit_owned` 有意只支持 Audio、Video 与 Byte Frame。

仓库已经提供可安装 Header、CMake Package 配置与独立 Consumer Example。

## 当前 Studio 边界

Studio 可以生成并保存 `node.cpp` 与 Manifest，但项目 Package 编译尚未启用。
后续 Host 必须创建 CMake 输入、编译稳定 ABI Library、校验 ABI 与精确 Factory
身份，并在 Package 可运行之前展示编译诊断。

Native 实现必须测试所有权、线程亲和性、取消后的回调、Buffer 生命周期、错误与
有界关闭。
