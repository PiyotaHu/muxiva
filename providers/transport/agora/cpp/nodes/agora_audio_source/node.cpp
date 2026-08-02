#include <voxa/agora_rtc.hpp>
#include <voxa/voxa.hpp>

#include <cstdlib>
#include <deque>
#include <mutex>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0') throw std::runtime_error(std::string("missing ") + name);
  return value;
}

struct OwnedAudio {
  std::vector<std::uint8_t> bytes;
  std::uint32_t sample_rate_hz = 0;
  std::uint16_t channels = 0;
  std::uint64_t samples_per_channel = 0;
  std::int64_t timestamp_ms = 0;
};

class AgoraAudioSourceNode final : public voxa::MultimodalGraphNode,
                                   private voxa::agora::SdkObserver {
 public:
  void on_prepare() override {
    sdk_ = voxa::agora::make_native_sdk();
    if (!sdk_) throw std::runtime_error("Agora Native SDK is not enabled in this build");
    const auto app_id = required_env("VOXA_AGORA_APP_ID");
    const auto token = required_env("VOXA_AGORA_SOURCE_TOKEN");
    const auto channel = required_env("VOXA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(std::stoul(required_env("VOXA_AGORA_SOURCE_UID")));
    if (sdk_->initialize(app_id, this) != 0 || sdk_->join(token, channel, uid) != 0) {
      sdk_->shutdown();
      throw std::runtime_error("Agora C++ SDK failed to join the configured room");
    }
  }

  void on_process(const voxa_frame_view_v1*, voxa::GraphNodeContext& ctx) override {
    OwnedAudio audio;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (queue_.empty()) return;
      audio = std::move(queue_.front());
      queue_.pop_front();
    }
    current_ = std::move(audio);
    frame_ = {};
    frame_.header.abi_version = VOXA_ABI_VERSION_V1;
    frame_.header.struct_size = sizeof(frame_.header);
    frame_.header.frame_type = VOXA_FRAME_AUDIO;
    frame_.header.clock_kind = VOXA_CLOCK_MONOTONIC;
    frame_.header.timestamp_ns = current_.timestamp_ms * 1000000;
    frame_.header.sequence_id = ++sequence_;
    frame_.payload.audio.sample_rate_hz = current_.sample_rate_hz;
    frame_.payload.audio.channels = current_.channels;
    frame_.payload.audio.sample_format = VOXA_PCM_I16LE;
    frame_.payload.audio.layout = VOXA_AUDIO_INTERLEAVED;
    frame_.payload.audio.samples_per_channel = current_.samples_per_channel;
    frame_.payload.audio.bytes = {current_.bytes.data(), current_.bytes.size()};
    ctx.emit("audio_out", frame_);
  }

  void on_finish() override {
    if (sdk_) {
      sdk_->leave();
      sdk_->shutdown();
      sdk_.reset();
    }
  }

  void on_abort(const voxa_abort_reason_v1&) noexcept override {
    try { on_finish(); } catch (...) {}
  }

 private:
  void on_audio_frame(const voxa::agora::Pcm16FrameView& frame) noexcept override {
    try {
      if (frame.data == nullptr || frame.size == 0 || frame.size > 256U * 1024U) return;
      OwnedAudio owned{{frame.data, frame.data + frame.size}, frame.sample_rate_hz,
                       frame.channels, frame.samples_per_channel, frame.timestamp_ms};
      std::lock_guard<std::mutex> lock(mutex_);
      if (queue_.size() == 256) queue_.pop_front();
      queue_.push_back(std::move(owned));
    } catch (...) {}
  }
  void on_connection_state(voxa::agora::ConnectionState, int) noexcept override {}
  void on_rejoined(std::uint32_t, int) noexcept override {}
  void on_connection_lost() noexcept override {}
  void on_token_expiring() noexcept override {}
  void on_token_required() noexcept override {}
  void on_network_quality(std::uint32_t, int, int) noexcept override {}
  void on_rtc_stats(const voxa::agora::RtcStatsSnapshot&) noexcept override {}
  void on_participant_joined(std::uint32_t) noexcept override {}
  void on_participant_left(std::uint32_t, int) noexcept override {}
  void on_error(int) noexcept override {}
  void on_video_frame(const voxa::agora::I420FrameView&) noexcept override {}

  std::unique_ptr<voxa::agora::Sdk> sdk_;
  std::mutex mutex_;
  std::deque<OwnedAudio> queue_;
  OwnedAudio current_;
  voxa_frame_view_v1 frame_{};
  std::uint64_t sequence_ = 0;
};
}  // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory = voxa::MultimodalGraphNodeFactory::make<AgoraAudioSourceNode>(
      "provider.agora.audio_source", VOXA_NODE_TRANSFORM,
      R"json([{"name":"tick_in","direction":"input","frameType":"event"},{"name":"audio_out","direction":"output","frameType":"audio"}])json");
  return factory.view();
}
