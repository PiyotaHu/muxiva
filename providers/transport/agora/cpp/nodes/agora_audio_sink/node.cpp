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

class AgoraAudioSinkNode final : public voxa::MultimodalGraphNode {
public:
  void on_prepare() override {
    const auto app_id = required_env("VOXA_AGORA_APP_ID");
    const auto token = required_env("VOXA_AGORA_BOT_TOKEN");
    const auto channel = required_env("VOXA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(
        std::stoul(required_env("VOXA_AGORA_BOT_UID")));
    const auto participant = static_cast<std::uint32_t>(
        std::stoul(required_env("VOXA_AGORA_WEB_UID")));
    session_ = voxa::agora::SharedSession::acquire(
        app_id, token, channel, uid, participant);
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
      if (input->header.sequence_id <= cancelled_through_sequence_) {
        dropped_bytes_.fetch_add(audio.bytes.len, std::memory_order_relaxed);
        return;
      }
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
    if (value != "voxa.voice.speech.started")
      return;
    std::size_t cancelled = 0;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      cancelled = pcm_.size();
      pcm_.clear();
      cancelled_through_sequence_ =
          std::max(cancelled_through_sequence_, signal.header.sequence_id);
      ++interruptions_;
    }
    std::fprintf(stderr,
                 "[VOXA][AGORA][audio.cancelled] signal=voxa.voice.speech.started "
                 "bytes=%zu through_sequence=%llu interruptions=%llu\n",
                 cancelled,
                 static_cast<unsigned long long>(signal.header.sequence_id),
                 static_cast<unsigned long long>(interruptions_.load()));
  }

  void on_finish() override {
    if (session_) {
      {
        std::lock_guard<std::mutex> lock(mutex_);
        stopping_ = true;
      }
      cv_.notify_all();
      if (sender_.joinable())
        sender_.join();
      session_.reset();
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
      const int result = session_ ? session_->send_audio(frame) : -7;
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
  std::shared_ptr<voxa::agora::SharedSession> session_;
  mutable std::mutex mutex_;
  std::condition_variable cv_;
  std::deque<std::uint8_t> pcm_;
  std::thread sender_;
  bool stopping_ = false;
  std::uint64_t published_packets_ = 0;
  std::uint64_t cancelled_through_sequence_ = 0;
  std::atomic<std::uint64_t> dropped_bytes_{0};
  std::atomic<std::uint64_t> interruptions_{0};
};
} // namespace

extern "C" voxa_multimodal_node_factory_v1 voxa_node_pack_factory() {
  static const auto factory = voxa::MultimodalGraphNodeFactory::make<
      AgoraAudioSinkNode>(
      "agora.audio_sink", VOXA_NODE_SINK,
      R"json([{"name":"audio_in","direction":"input","frameType":"audio"},{"name":"signal_in","direction":"input","frameType":"signal"}])json");
  return factory.view();
}
