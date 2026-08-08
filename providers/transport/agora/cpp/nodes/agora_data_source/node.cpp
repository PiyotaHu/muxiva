#include <muxiva/agora_rtc.hpp>
#include <muxiva/muxiva.hpp>

#include <chrono>
#include <cstdlib>
#include <stdexcept>
#include <string>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0')
    throw std::runtime_error(std::string("missing ") + name);
  return value;
}

muxiva_str_v1 borrow(const std::string& value) noexcept {
  return {value.data(), value.size()};
}

class AgoraDataSourceNode final : public muxiva::MultimodalGraphNode {
 public:
  void on_prepare() override {
    const auto uid = static_cast<std::uint32_t>(
        std::stoul(required_env("MUXIVA_AGORA_BOT_UID")));
    const auto participant = static_cast<std::uint32_t>(
        std::stoul(required_env("MUXIVA_AGORA_WEB_UID")));
    session_ = muxiva::agora::SharedSession::acquire(
        required_env("MUXIVA_AGORA_APP_ID"),
        required_env("MUXIVA_AGORA_BOT_TOKEN"),
        required_env("MUXIVA_AGORA_CHANNEL"), uid, participant);
  }

  void on_process(const muxiva_frame_view_v1*,
                  muxiva::GraphNodeContext& context) override {
    context.schedule_next_tick(std::chrono::milliseconds(20));
    if (!session_ || !session_->try_pop_data(current_)) return;
    frame_ = {};
    frame_.header.abi_version = MUXIVA_ABI_VERSION_V1;
    frame_.header.struct_size = sizeof(frame_.header);
    frame_.header.frame_type = MUXIVA_FRAME_BYTE;
    frame_.header.clock_kind = MUXIVA_CLOCK_MONOTONIC;
    frame_.header.timestamp_ns =
        static_cast<std::int64_t>(current_.sent_timestamp_ms) * 1000000;
    frame_.header.sequence_id = ++sequence_;
    frame_id_ = "agora-data-" + std::to_string(sequence_);
    stream_id_ = "agora-remote-" + std::to_string(current_.remote_uid);
    frame_.header.frame_id = borrow(frame_id_);
    frame_.header.clock_domain_id = borrow(clock_domain_);
    frame_.header.stream_id = borrow(stream_id_);
    frame_.header.trace_id = borrow(frame_id_);
    frame_.payload.bytes.bytes = {current_.bytes.data(), current_.bytes.size()};
    frame_.payload.bytes.media_type = borrow(media_type_);
    context.emit("message_out", frame_);
  }

  void on_finish() override { session_.reset(); }
  void on_abort(const muxiva_abort_reason_v1&) noexcept override {
    try { on_finish(); } catch (...) {}
  }

 private:
  std::shared_ptr<muxiva::agora::SharedSession> session_;
  muxiva::agora::OwnedDataMessage current_;
  muxiva_frame_view_v1 frame_{};
  std::uint64_t sequence_ = 0;
  std::string frame_id_;
  std::string stream_id_;
  std::string clock_domain_ = "agora.remote.monotonic";
  std::string media_type_ = "application/vnd.muxiva.client-command+json";
};
}  // namespace

extern "C" muxiva_multimodal_node_factory_v1 muxiva_node_pack_factory() {
  static const auto factory =
      muxiva::MultimodalGraphNodeFactory::make<AgoraDataSourceNode>(
          "agora.data_source", MUXIVA_NODE_SOURCE,
          R"json([{"name":"message_out","direction":"output","frameType":"byte"}])json",
          "{}", "1.0.0");
  return factory.view();
}
