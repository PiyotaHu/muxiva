#include <voxa/agora_rtc.hpp>
#include <voxa/voxa.hpp>

#include <cstdlib>
#include <stdexcept>
#include <string>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0') throw std::runtime_error(std::string("missing ") + name);
  return value;
}

class Observer final : public voxa::agora::SdkObserver {
 public:
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
  void on_audio_frame(const voxa::agora::Pcm16FrameView&) noexcept override {}
  void on_video_frame(const voxa::agora::I420FrameView&) noexcept override {}
};

class AgoraAudioSinkNode final : public voxa::MultimodalGraphNode {
 public:
  void on_prepare() override {
    sdk_ = voxa::agora::make_native_sdk();
    if (!sdk_) throw std::runtime_error("Agora Native SDK is not enabled in this build");
    const auto app_id = required_env("VOXA_AGORA_APP_ID");
    const auto token = required_env("VOXA_AGORA_BOT_TOKEN");
    const auto channel = required_env("VOXA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(std::stoul(required_env("VOXA_AGORA_BOT_UID")));
    if (sdk_->initialize(app_id, &observer_) != 0 || sdk_->join(token, channel, uid) != 0) {
      sdk_->shutdown();
      throw std::runtime_error("Agora C++ SDK failed to join the configured room");
    }
  }

  void on_process(const voxa_frame_view_v1* input, voxa::GraphNodeContext&) override {
    if (input == nullptr || input->header.frame_type != VOXA_FRAME_AUDIO) {
      throw std::invalid_argument("Agora audio Sink requires an Audio Frame");
    }
    const auto& audio = input->payload.audio;
    const voxa::agora::Pcm16FrameView view{
        audio.bytes.data, audio.bytes.len, audio.sample_rate_hz, audio.channels,
        audio.samples_per_channel, input->header.timestamp_ns / 1000000, 0};
    if (sdk_->push_audio(view) != 0) throw std::runtime_error("Agora audio publish failed");
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
  Observer observer_;
  std::unique_ptr<voxa::agora::Sdk> sdk_;
};
}  // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory = voxa::MultimodalGraphNodeFactory::make<AgoraAudioSinkNode>(
      "provider.agora.audio_sink", VOXA_NODE_SINK,
      R"json([{"name":"audio_in","direction":"input","frame_type":"audio"}])json");
  return factory.view();
}
