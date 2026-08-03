#include <voxa/agora_rtc.hpp>
#include <voxa/voxa.hpp>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <string>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0') throw std::runtime_error(std::string("missing ") + name);
  return value;
}

voxa_str_v1 borrow(const std::string& value) noexcept {
  return {value.data(), value.size()};
}

class AgoraAudioSourceNode final : public voxa::MultimodalGraphNode {
 public:
  void on_prepare() override {
    const auto app_id = required_env("VOXA_AGORA_APP_ID");
    const auto token = required_env("VOXA_AGORA_BOT_TOKEN");
    const auto channel = required_env("VOXA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(std::stoul(required_env("VOXA_AGORA_BOT_UID")));
    const auto participant = static_cast<std::uint32_t>(
        std::stoul(required_env("VOXA_AGORA_WEB_UID")));
    session_ = voxa::agora::SharedSession::acquire(
        app_id, token, channel, uid, participant);
  }

  void on_process(const voxa_frame_view_v1*, voxa::GraphNodeContext& ctx) override {
    ctx.schedule_next_tick(std::chrono::milliseconds(20));
    if (!session_ || !session_->try_pop_audio(current_)) return;
    frame_ = {};
    frame_.header.abi_version = VOXA_ABI_VERSION_V1;
    frame_.header.struct_size = sizeof(frame_.header);
    frame_.header.frame_type = VOXA_FRAME_AUDIO;
    frame_.header.clock_kind = VOXA_CLOCK_MONOTONIC;
    frame_.header.timestamp_ns = current_.timestamp_ms * 1000000;
    frame_.header.sequence_id = ++sequence_;
    frame_id_ = "agora-audio-" + std::to_string(sequence_);
    stream_id_ = "agora-remote-" + std::to_string(current_.remote_uid);
    trace_id_ = frame_id_;
    frame_.header.frame_id = borrow(frame_id_);
    frame_.header.clock_domain_id = borrow(clock_domain_);
    frame_.header.stream_id = borrow(stream_id_);
    frame_.header.trace_id = borrow(trace_id_);
    frame_.payload.audio.sample_rate_hz = current_.sample_rate_hz;
    frame_.payload.audio.channels = current_.channels;
    frame_.payload.audio.sample_format = VOXA_PCM_I16LE;
    frame_.payload.audio.layout = VOXA_AUDIO_INTERLEAVED;
    frame_.payload.audio.samples_per_channel = current_.samples_per_channel;
    frame_.payload.audio.bytes = {current_.bytes.data(), current_.bytes.size()};
    ctx.emit("audio_out", frame_);
    const auto emitted = ++emitted_frames_;
    if (emitted == 1 || emitted % 500 == 0) {
      std::fprintf(stderr,
                   "[VOXA][AGORA][audio.forwarded] frames=%llu bytes=%zu "
                   "participant_uid=%u\n",
                   static_cast<unsigned long long>(emitted), current_.bytes.size(),
                   current_.remote_uid);
    }
  }

  void on_finish() override {
    session_.reset();
  }

  void on_abort(const voxa_abort_reason_v1&) noexcept override {
    try { on_finish(); } catch (...) {}
  }

 private:
  std::shared_ptr<voxa::agora::SharedSession> session_;
  voxa::agora::OwnedPcm16Frame current_;
  voxa_frame_view_v1 frame_{};
  std::uint64_t sequence_ = 0;
  std::atomic<std::uint64_t> emitted_frames_{0};
  std::string frame_id_;
  std::string clock_domain_ = "agora.remote.monotonic";
  std::string stream_id_;
  std::string trace_id_;
};
}  // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory = voxa::MultimodalGraphNodeFactory::make<AgoraAudioSourceNode>(
      "agora.audio_source", VOXA_NODE_SOURCE,
      R"json([{"name":"audio_out","direction":"output","frameType":"audio"}])json",
      "{}", "1.1.0");
  return factory.view();
}
