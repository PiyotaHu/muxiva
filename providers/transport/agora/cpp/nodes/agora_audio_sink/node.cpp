#include <voxa/agora_rtc.hpp>
#include <voxa/voxa.hpp>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <deque>
#include <mutex>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {
std::string required_env(const char *name) {
  const char *value = std::getenv(name);
  if (value == nullptr || *value == '\0')
    throw std::runtime_error(std::string("missing ") + name);
  return value;
}

class Observer final : public voxa::agora::SdkObserver {
public:
  void on_connection_state(voxa::agora::ConnectionState,
                           int) noexcept override {}
  void on_rejoined(std::uint32_t, int) noexcept override {}
  void on_connection_lost() noexcept override {}
  void on_token_expiring() noexcept override {}
  void on_token_required() noexcept override {}
  void on_network_quality(std::uint32_t, int, int) noexcept override {}
  void on_rtc_stats(const voxa::agora::RtcStatsSnapshot &) noexcept override {}
  void on_participant_joined(std::uint32_t) noexcept override {}
  void on_participant_left(std::uint32_t, int) noexcept override {}
  void on_error(int) noexcept override {}
  void on_audio_frame(const voxa::agora::Pcm16FrameView &) noexcept override {}
  void on_video_frame(const voxa::agora::I420FrameView &) noexcept override {}
};

class AgoraAudioSinkNode final : public voxa::MultimodalGraphNode {
public:
  void on_prepare() override {
    sdk_ = voxa::agora::make_native_sdk();
    if (!sdk_)
      throw std::runtime_error("Agora Native SDK is not enabled in this build");
    const auto app_id = required_env("VOXA_AGORA_APP_ID");
    const auto token = required_env("VOXA_AGORA_BOT_TOKEN");
    const auto channel = required_env("VOXA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(
        std::stoul(required_env("VOXA_AGORA_BOT_UID")));
    if (sdk_->initialize(app_id, &observer_) != 0 ||
        sdk_->join(token, channel, uid) != 0) {
      sdk_->shutdown();
      throw std::runtime_error(
          "Agora C++ SDK failed to join the configured room");
    }
    sender_ = std::thread([this] { send_loop(); });
  }

  void on_process(const voxa_frame_view_v1 *input,
                  voxa::GraphNodeContext &) override {
    if (input == nullptr || input->header.frame_type != VOXA_FRAME_AUDIO) {
      throw std::invalid_argument("Agora audio Sink requires an Audio Frame");
    }
    const auto &audio = input->payload.audio;
    if (audio.sample_rate_hz != kSampleRate || audio.channels != 1 ||
        audio.bytes.data == nullptr || audio.bytes.len == 0 ||
        audio.bytes.len % 2 != 0) {
      throw std::invalid_argument(
          "Agora audio Sink requires non-empty 48kHz mono PCM s16le");
    }
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (pcm_.size() + audio.bytes.len > kMaximumQueuedBytes)
        throw std::runtime_error(
            "Agora audio Sink exceeded its 120 second safety buffer");
      pcm_.insert(pcm_.end(), audio.bytes.data,
                  audio.bytes.data + audio.bytes.len);
    }
    cv_.notify_one();
  }

  void on_signal(const voxa_frame_view_v1 &signal) override {
    if (signal.header.frame_type != VOXA_FRAME_SIGNAL)
      return;
    const auto &name = signal.payload.signal.signal_name;
    const std::string_view value(name.data == nullptr ? "" : name.data,
                                 name.data == nullptr ? 0 : name.len);
    if (value != "voxa.runtime.interrupt")
      return;
    std::size_t cancelled = 0;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      cancelled = pcm_.size();
      pcm_.clear();
      ++interruptions_;
    }
    std::fprintf(stderr,
                 "[VOXA][AGORA][audio.cancelled] signal=voxa.runtime.interrupt "
                 "bytes=%zu interruptions=%llu\n",
                 cancelled,
                 static_cast<unsigned long long>(interruptions_.load()));
  }

  void on_finish() override {
    if (sdk_) {
      {
        std::lock_guard<std::mutex> lock(mutex_);
        stopping_ = true;
      }
      cv_.notify_all();
      if (sender_.joinable())
        sender_.join();
      sdk_->leave();
      sdk_->shutdown();
      sdk_.reset();
    }
  }

  void on_abort(const voxa_abort_reason_v1 &) noexcept override {
    try {
      on_finish();
    } catch (...) {
    }
  }

private:
  void send_loop() noexcept {
    using Clock = std::chrono::steady_clock;
    auto next = Clock::now();
    std::vector<std::uint8_t> packet(kPacketBytes);
    for (;;) {
      {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_.wait(lock,
                 [this] { return stopping_ || pcm_.size() >= kPacketBytes; });
        if (pcm_.size() < kPacketBytes) {
          if (stopping_)
            return;
          continue;
        }
        for (std::size_t index = 0; index < kPacketBytes; ++index) {
          packet[index] = pcm_.front();
          pcm_.pop_front();
        }
      }
      const voxa::agora::Pcm16FrameView frame{packet.data(),
                                              packet.size(),
                                              kSampleRate,
                                              1,
                                              kSamplesPerPacket,
                                              0,
                                              0};
      const int result = sdk_->push_audio(frame);
      const auto count = ++published_packets_;
      if (count == 1 || count % 100 == 0 || result != 0) {
        std::fprintf(stderr,
                     "[VOXA][AGORA][audio.published] packets=%llu result=%d "
                     "queued_bytes=%zu dropped_bytes=%llu\n",
                     static_cast<unsigned long long>(count), result,
                     queued_bytes(),
                     static_cast<unsigned long long>(
                         dropped_bytes_.load(std::memory_order_relaxed)));
      }
      next += std::chrono::milliseconds(10);
      if (next < Clock::now() - std::chrono::milliseconds(100))
        next = Clock::now();
      std::this_thread::sleep_until(next);
    }
  }

  std::size_t queued_bytes() const noexcept {
    std::lock_guard<std::mutex> lock(mutex_);
    return pcm_.size();
  }

  static constexpr std::uint32_t kSampleRate = 48000;
  static constexpr std::uint64_t kSamplesPerPacket = 480;
  static constexpr std::size_t kPacketBytes = kSamplesPerPacket * 2;
  static constexpr std::size_t kMaximumQueuedBytes = kSampleRate * 2 * 120;
  Observer observer_;
  std::unique_ptr<voxa::agora::Sdk> sdk_;
  mutable std::mutex mutex_;
  std::condition_variable cv_;
  std::deque<std::uint8_t> pcm_;
  std::thread sender_;
  bool stopping_ = false;
  std::uint64_t published_packets_ = 0;
  std::atomic<std::uint64_t> dropped_bytes_{0};
  std::atomic<std::uint64_t> interruptions_{0};
};
} // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory = voxa::MultimodalGraphNodeFactory::make<
      AgoraAudioSinkNode>(
      "provider.agora.audio_sink", VOXA_NODE_SINK,
      R"json([{"name":"audio_in","direction":"input","frameType":"audio"}])json");
  return factory.view();
}
