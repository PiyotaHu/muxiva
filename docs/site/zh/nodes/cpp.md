# C++ Node

C++ SDK 在 Voxa 版本化 C ABI 之上提供 RAII Wrapper。多模态实现通过声明的
Port 输出具名 Frame。

```cpp
#include <voxa/voxa.hpp>

class MyNode final : public voxa::MultimodalGraphNode {
 public:
  void on_process(const voxa_frame_view_v1* input,
                  voxa::GraphNodeContext& ctx) override {
    // ctx.emit("text_out", output_frame);
    // Source 可调用 ctx.schedule_next_tick(std::chrono::milliseconds(20));
    // 自己安排下一次轮询，不需要在 Graph 里连接时钟 Node。
  }
  void on_signal(const voxa_frame_view_v1& signal) override {
    // 接收 voxa.voice.speech.started 等图内控制信号。
  }
};
```

旧的 `std::vector<GraphEmission>` 返回 Hook 继续保持源码兼容，新 Node 应通过
Context 显式发送。V1 C ABI 已支持通过 `on_signal` 接收 Signal；从 C++ 主动发送
Signal 或 EventBus 事件仍需要后续控制动作 Context 扩展。

仓库已经提供可安装 Header、CMake Package 配置与独立 Consumer Example。

## 当前 Studio 边界

Studio 可以生成并保存 `node.cpp` 与 Manifest，但项目 Package 编译尚未启用。
后续 Host 必须创建 CMake 输入、编译稳定 ABI Library、校验 ABI 与精确 Factory
身份，并在 Package 可运行之前展示编译诊断。

Native 实现必须测试所有权、线程亲和性、取消后的回调、Buffer 生命周期、错误与
有界关闭。
