#include <voxa/agora_rtc.hpp>
#include <voxa/voxa.hpp>

#include <atomic>
#include <cstdio>
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
  std::uint32_t remote_uid = 0;
};

voxa_str_v1 borrow(const std::string& value) noexcept {
  return {value.data(), value.size()};
}

class AgoraAudioSourceNode final : public voxa::MultimodalGraphNode,
                                   private voxa::agora::SdkObserver {
 public:
  void on_prepare() override {
    sdk_ = voxa::agora::make_native_sdk();
    if (!sdk_) throw std::runtime_error("Agora Native SDK is not enabled in this build");
    const auto app_id = required_env("VOXA_AGORA_APP_ID");
    const auto token = required_env("VOXA_AGORA_BOT_TOKEN");
    const auto channel = required_env("VOXA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(std::stoul(required_env("VOXA_AGORA_BOT_UID")));
    if (sdk_->initialize(app_id, this) != 0 || sdk_->join(token, channel, uid) != 0) {
      sdk_->shutdown();
      throw std::runtime_error("Agora C++ SDK failed to join the configured room");
    }
  }

  void on_process(const voxa_frame_view_v1*, voxa::GraphNodeContext& ctx) override {
    OwnedAudio audio;
    std::size_t combined_frames = 0;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (queue_.empty()) return;
      audio = std::move(queue_.front());
      queue_.pop_front();
      combined_frames = 1;
      const auto maximum_samples =
          static_cast<std::uint64_t>(audio.sample_rate_hz) * 40 / 1000;
      while (!queue_.empty() && combined_frames < 8) {
        const auto& next = queue_.front();
        if (next.sample_rate_hz != audio.sample_rate_hz ||
            next.channels != audio.channels || next.remote_uid != audio.remote_uid ||
            audio.samples_per_channel + next.samples_per_channel > maximum_samples) {
          break;
        }
        audio.bytes.insert(audio.bytes.end(), next.bytes.begin(), next.bytes.end());
        audio.samples_per_channel += next.samples_per_channel;
        queue_.pop_front();
        ++combined_frames;
      }
    }
    current_ = std::move(audio);
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
                   "combined=%zu received=%llu dropped=%llu\n",
                   static_cast<unsigned long long>(emitted), current_.bytes.size(),
                   combined_frames,
                   static_cast<unsigned long long>(received_frames_.load()),
                   static_cast<unsigned long long>(dropped_frames_.load()));
    }
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
                       frame.channels, frame.samples_per_channel, frame.timestamp_ms,
                       frame.remote_uid};
      std::lock_guard<std::mutex> lock(mutex_);
      ++received_frames_;
      if (queue_.size() == 512) {
        queue_.pop_front();
        ++dropped_frames_;
      }
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
  std::atomic<std::uint64_t> received_frames_{0};
  std::atomic<std::uint64_t> emitted_frames_{0};
  std::atomic<std::uint64_t> dropped_frames_{0};
  std::string frame_id_;
  std::string clock_domain_ = "agora.remote.monotonic";
  std::string stream_id_;
  std::string trace_id_;
};
}  // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory = voxa::MultimodalGraphNodeFactory::make<AgoraAudioSourceNode>(
      "provider.agora.audio_source", VOXA_NODE_TRANSFORM,
      R"json([{"name":"tick_in","direction":"input","frameType":"event"},{"name":"audio_out","direction":"output","frameType":"audio"}])json");
  return factory.view();
}
