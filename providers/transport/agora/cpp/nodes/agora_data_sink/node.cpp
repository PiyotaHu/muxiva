#include <voxa/agora_rtc.hpp>
#include <voxa/voxa.hpp>

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <string>
#include <thread>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0')
    throw std::runtime_error(std::string("missing ") + name);
  return value;
}

class AgoraDataSinkNode final : public voxa::MultimodalGraphNode {
 public:
  void on_prepare() override {
    const auto uid = static_cast<std::uint32_t>(
        std::stoul(required_env("VOXA_AGORA_BOT_UID")));
    const auto participant = static_cast<std::uint32_t>(
        std::stoul(required_env("VOXA_AGORA_WEB_UID")));
    session_ = voxa::agora::SharedSession::acquire(
        required_env("VOXA_AGORA_APP_ID"),
        required_env("VOXA_AGORA_BOT_TOKEN"),
        required_env("VOXA_AGORA_CHANNEL"), uid, participant);
    next_send_ = std::chrono::steady_clock::now();
  }

  void on_process(const voxa_frame_view_v1* input,
                  voxa::GraphNodeContext&) override {
    if (input == nullptr || input->header.frame_type != VOXA_FRAME_BYTE)
      throw std::invalid_argument("Agora data Sink requires a Byte Frame");
    const auto& payload = input->payload.bytes.bytes;
    if (payload.data == nullptr || payload.len == 0 || payload.len > 1024)
      throw std::invalid_argument(
          "Agora data Sink requires a message from 1 through 1024 bytes");
    const auto now = std::chrono::steady_clock::now();
    if (next_send_ > now) std::this_thread::sleep_until(next_send_);
    const int result = session_ ? session_->send_data(payload.data, payload.len) : -7;
    const auto count = ++published_;
    if (count == 1 || count % 25 == 0 || result != 0) {
      std::fprintf(stderr,
                   "[VOXA][AGORA][data.published] messages=%llu bytes=%zu result=%d\n",
                   static_cast<unsigned long long>(count), payload.len, result);
    }
    if (result != 0)
      throw std::runtime_error("Agora data Sink failed to publish message: code=" +
                               std::to_string(result));

    // Pace by bytes below Agora's 6 KiB/s and 30-call/s ceilings. Small
    // transcript deltas stay responsive while 1 KiB fragments remain safe.
    constexpr std::size_t kBudgetBytesPerSecond = 5U * 1024U;
    constexpr std::size_t kMaximumCallsPerSecond = 25U;
    const auto byte_delay_ms = static_cast<std::int64_t>(
        (payload.len * 1000U + kBudgetBytesPerSecond - 1U) /
        kBudgetBytesPerSecond);
    const auto call_delay_ms = static_cast<std::int64_t>(
        (1000U + kMaximumCallsPerSecond - 1U) / kMaximumCallsPerSecond);
    next_send_ = std::chrono::steady_clock::now() + std::chrono::milliseconds(
        std::max(byte_delay_ms, call_delay_ms));
  }

  void on_finish() override { session_.reset(); }

  void on_abort(const voxa_abort_reason_v1&) noexcept override {
    try { on_finish(); } catch (...) {}
  }

 private:
  std::shared_ptr<voxa::agora::SharedSession> session_;
  std::chrono::steady_clock::time_point next_send_{};
  std::uint64_t published_ = 0;
};
}  // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory =
      voxa::MultimodalGraphNodeFactory::make<AgoraDataSinkNode>(
          "agora.data_sink", VOXA_NODE_SINK,
          R"json([{"name":"message_in","direction":"input","frameType":"byte"}])json",
          "{}", "1.0.0");
  return factory.view();
}
