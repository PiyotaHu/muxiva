#include "voxa/agora_rtc.hpp"

#include "IAgoraMediaEngine.h"
#include "IAgoraRtcEngine.h"

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <deque>
#include <functional>
#include <future>
#include <memory>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

namespace voxa::agora {
namespace {

class SerialExecutor final {
 public:
  SerialExecutor() : worker_([this] { run(); }) {}
  SerialExecutor(const SerialExecutor&) = delete;
  SerialExecutor& operator=(const SerialExecutor&) = delete;
  ~SerialExecutor() { stop(); }

  int call(std::function<int()> action) noexcept {
    try {
      auto task = std::make_shared<std::packaged_task<int()>>(std::move(action));
      auto future = task->get_future();
      {
        std::lock_guard<std::mutex> lock(mutex_);
        if (stopping_) return -7;
        queue_.emplace_back([task] { (*task)(); });
      }
      cv_.notify_one();
      return future.get();
    } catch (...) {
      return -1;
    }
  }

  void stop() noexcept {
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (stopping_) return;
      stopping_ = true;
    }
    cv_.notify_all();
    if (worker_.joinable()) worker_.join();
  }

 private:
  void run() noexcept {
    for (;;) {
      std::function<void()> action;
      {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_.wait(lock, [&] { return stopping_ || !queue_.empty(); });
        if (queue_.empty() && stopping_) return;
        action = std::move(queue_.front());
        queue_.pop_front();
      }
      action();
    }
  }

  std::mutex mutex_;
  std::condition_variable cv_;
  std::deque<std::function<void()>> queue_;
  bool stopping_ = false;
  std::thread worker_;
};

class NativeSdk final : public Sdk,
                        private ::agora::rtc::IRtcEngineEventHandler,
                        private ::agora::media::IAudioFrameObserver,
                        private ::agora::media::IVideoFrameObserver {
 public:
  ~NativeSdk() override { shutdown(); }

  int initialize(const std::string& app_id, SdkObserver* observer) noexcept override {
    try {
      return executor_.call([this, app_id, observer] {
        if (engine_ != nullptr || observer == nullptr) return -2;
        observer_.store(observer, std::memory_order_release);
        engine_ = ::createAgoraRtcEngine();
        if (engine_ == nullptr) return -1;
        ::agora::rtc::RtcEngineContext context;
        context.appId = app_id.c_str();
        context.eventHandler = this;
        context.channelProfile = ::agora::CHANNEL_PROFILE_COMMUNICATION;
        int result = engine_->initialize(context);
        if (result != 0) return result;
        void* media = nullptr;
        result = engine_->queryInterface(::agora::rtc::AGORA_IID_MEDIA_ENGINE, &media);
        if (result != 0 || media == nullptr) return result == 0 ? -1 : result;
        media_ = static_cast<::agora::media::IMediaEngine*>(media);
        if ((result = media_->registerAudioFrameObserver(this)) != 0) return result;
        if ((result = media_->registerVideoFrameObserver(this)) != 0) return result;
        ::agora::rtc::AudioTrackConfig audio_config;
        audio_config.enableLocalPlayback = false;
        audio_track_ = media_->createCustomAudioTrack(
            ::agora::rtc::AUDIO_TRACK_DIRECT, audio_config);
        if (audio_track_ == kInvalidTrack) return -1;
        video_track_ = engine_->createCustomVideoTrack();
        if (video_track_ == kInvalidTrack) return -1;
        return engine_->enableVideo();
      });
    } catch (...) {
      return -1;
    }
  }

  int join(const std::string& token, const std::string& channel,
           std::uint32_t uid) noexcept override {
    try {
      return executor_.call([this, token, channel, uid] {
        if (engine_ == nullptr) return -7;
        ::agora::rtc::ChannelMediaOptions options;
        options.publishMicrophoneTrack = false;
        options.publishCameraTrack = false;
        options.publishCustomAudioTrack = true;
        options.publishCustomAudioTrackId = audio_track_;
        options.publishCustomVideoTrack = true;
        options.customVideoTrackId = video_track_;
        return engine_->joinChannel(token.empty() ? nullptr : token.c_str(),
                                    channel.c_str(), uid, options);
      });
    } catch (...) {
      return -1;
    }
  }

  int leave() noexcept override {
    return executor_.call([this] { return engine_ == nullptr ? 0 : engine_->leaveChannel(); });
  }

  int renew_token(const std::string& token) noexcept override {
    try {
      return executor_.call([this, token] {
        return engine_ == nullptr ? -7 : engine_->renewToken(token.c_str());
      });
    } catch (...) {
      return -1;
    }
  }

  int push_audio(const Pcm16FrameView& value) noexcept override {
    try {
      std::vector<std::uint8_t> bytes(value.data, value.data + value.size);
      return executor_.call([this, bytes = std::move(bytes), value]() mutable {
        if (media_ == nullptr || value.sample_rate_hz != 48000 || value.channels != 1) {
          return -2;
        }
        ::agora::media::IAudioFrameObserverBase::AudioFrame frame;
        frame.type = ::agora::media::IAudioFrameObserverBase::FRAME_TYPE_PCM16;
        frame.samplesPerChannel = static_cast<int>(value.samples_per_channel);
        frame.bytesPerSample = ::agora::rtc::TWO_BYTES_PER_SAMPLE;
        frame.channels = value.channels;
        frame.samplesPerSec = value.sample_rate_hz;
        frame.buffer = bytes.data();
        frame.renderTimeMs = value.timestamp_ms;
        return media_->pushAudioFrame(&frame, audio_track_);
      });
    } catch (...) {
      return -1;
    }
  }

  int push_video(const I420FrameView& value) noexcept override {
    try {
      const auto pixels = static_cast<std::size_t>(value.width) * value.height;
      std::vector<std::uint8_t> bytes(pixels + pixels / 2);
      for (std::uint32_t row = 0; row < value.height; ++row) {
        std::copy_n(value.y + static_cast<std::size_t>(row) * value.y_stride,
                    value.width, bytes.data() + static_cast<std::size_t>(row) * value.width);
      }
      auto* u = bytes.data() + pixels;
      auto* v = u + pixels / 4;
      for (std::uint32_t row = 0; row < value.height / 2; ++row) {
        std::copy_n(value.u + static_cast<std::size_t>(row) * value.u_stride,
                    value.width / 2, u + static_cast<std::size_t>(row) * value.width / 2);
        std::copy_n(value.v + static_cast<std::size_t>(row) * value.v_stride,
                    value.width / 2, v + static_cast<std::size_t>(row) * value.width / 2);
      }
      return executor_.call([this, bytes = std::move(bytes), value]() mutable {
        if (media_ == nullptr) return -7;
        ::agora::media::base::ExternalVideoFrame frame;
        frame.type = ::agora::media::base::ExternalVideoFrame::VIDEO_BUFFER_RAW_DATA;
        frame.format = ::agora::media::base::VIDEO_PIXEL_I420;
        frame.buffer = bytes.data();
        frame.stride = static_cast<int>(value.width);
        frame.height = static_cast<int>(value.height);
        frame.timestamp = value.timestamp_ms;
        return media_->pushVideoFrame(&frame, video_track_);
      });
    } catch (...) {
      return -1;
    }
  }

  void shutdown() noexcept override {
    if (shutdown_) return;
    shutdown_ = true;
    (void)executor_.call([this] {
      if (media_ != nullptr) {
        (void)media_->registerAudioFrameObserver(nullptr);
        (void)media_->registerVideoFrameObserver(nullptr);
        if (audio_track_ != kInvalidTrack) {
          (void)media_->destroyCustomAudioTrack(audio_track_);
          audio_track_ = kInvalidTrack;
        }
        media_ = nullptr;
      }
      observer_.store(nullptr, std::memory_order_release);
      if (engine_ != nullptr) {
        if (video_track_ != kInvalidTrack) {
          (void)engine_->destroyCustomVideoTrack(video_track_);
          video_track_ = kInvalidTrack;
        }
        ::agora::rtc::IRtcEngine::release();
        engine_ = nullptr;
      }
      return 0;
    });
    executor_.stop();
  }

 private:
  using AudioFrame = ::agora::media::IAudioFrameObserverBase::AudioFrame;
  using AudioParams = ::agora::media::IAudioFrameObserverBase::AudioParams;
  using VideoFrame = ::agora::media::base::VideoFrame;

  bool onRecordAudioFrame(const char*, AudioFrame&) override { return true; }
  bool onPlaybackAudioFrame(const char*, AudioFrame&) override { return true; }
  bool onMixedAudioFrame(const char*, AudioFrame&) override { return true; }
  bool onEarMonitoringAudioFrame(AudioFrame&) override { return true; }
  bool onPlaybackAudioFrameBeforeMixing(const char*, ::agora::rtc::uid_t uid,
                                        AudioFrame& frame) override {
    auto* observer = observer_.load(std::memory_order_acquire);
    if (observer != nullptr && frame.buffer != nullptr && frame.samplesPerChannel > 0 &&
        frame.channels > 0 && frame.bytesPerSample == ::agora::rtc::TWO_BYTES_PER_SAMPLE) {
      const auto size = static_cast<std::size_t>(frame.samplesPerChannel) * frame.channels * 2;
      observer->on_audio_frame(
          {static_cast<const std::uint8_t*>(frame.buffer), size,
           static_cast<std::uint32_t>(frame.samplesPerSec),
           static_cast<std::uint16_t>(frame.channels),
           static_cast<std::uint64_t>(frame.samplesPerChannel), frame.renderTimeMs, uid});
    }
    return true;
  }
  int getObservedAudioFramePosition() override {
    return AUDIO_FRAME_POSITION_BEFORE_MIXING;
  }
  AudioParams getPlaybackAudioParams() override { return {}; }
  AudioParams getRecordAudioParams() override { return {}; }
  AudioParams getMixedAudioParams() override { return {}; }
  AudioParams getEarMonitoringAudioParams() override { return {}; }

  bool onCaptureVideoFrame(::agora::rtc::VIDEO_SOURCE_TYPE, VideoFrame&) override {
    return true;
  }
  bool onPreEncodeVideoFrame(::agora::rtc::VIDEO_SOURCE_TYPE, VideoFrame&) override {
    return true;
  }
  bool onMediaPlayerVideoFrame(VideoFrame&, int) override { return true; }
  bool onRenderVideoFrame(const char*, ::agora::rtc::uid_t uid,
                          VideoFrame& frame) override {
    auto* observer = observer_.load(std::memory_order_acquire);
    if (observer != nullptr && frame.type == ::agora::media::base::VIDEO_PIXEL_I420 &&
        frame.width > 0 && frame.height > 0) {
      observer->on_video_frame(
          {frame.yBuffer, frame.uBuffer, frame.vBuffer,
           static_cast<std::size_t>(frame.yStride),
           static_cast<std::size_t>(frame.uStride),
           static_cast<std::size_t>(frame.vStride),
           static_cast<std::uint32_t>(frame.width),
           static_cast<std::uint32_t>(frame.height), frame.renderTimeMs, uid});
    }
    return true;
  }
  bool onTranscodedVideoFrame(VideoFrame&) override { return true; }
  ::agora::media::IVideoFrameObserver::VIDEO_FRAME_PROCESS_MODE
  getVideoFrameProcessMode() override {
    return PROCESS_MODE_READ_ONLY;
  }
  ::agora::media::base::VIDEO_PIXEL_FORMAT getVideoFormatPreference() override {
    return ::agora::media::base::VIDEO_PIXEL_I420;
  }
  std::uint32_t getObservedFramePosition() override {
    return ::agora::media::base::POSITION_PRE_RENDERER;
  }

  void onConnectionStateChanged(::agora::rtc::CONNECTION_STATE_TYPE state,
                                ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_connection_state(static_cast<ConnectionState>(state),
                                    static_cast<int>(reason));
    }
  }
  void onRejoinChannelSuccess(const char*, ::agora::rtc::uid_t uid,
                              int elapsed) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_rejoined(uid, elapsed);
    }
  }
  void onConnectionLost() override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_connection_lost();
    }
  }
  void onTokenPrivilegeWillExpire(const char*) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_token_expiring();
    }
  }
  void onRequestToken() override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_token_required();
    }
  }
  void onNetworkQuality(::agora::rtc::uid_t uid, int tx_quality,
                        int rx_quality) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_network_quality(uid, tx_quality, rx_quality);
    }
  }
  void onRtcStats(const ::agora::rtc::RtcStats& stats) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_rtc_stats(
          {stats.duration, stats.txBytes, stats.rxBytes, stats.userCount,
           stats.lastmileDelay});
    }
  }
  void onUserJoined(::agora::rtc::uid_t uid, int) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_participant_joined(uid);
    }
  }
  void onUserOffline(::agora::rtc::uid_t uid,
                     ::agora::rtc::USER_OFFLINE_REASON_TYPE reason) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_participant_left(uid, static_cast<int>(reason));
    }
  }
  void onError(int error, const char*) override {
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_error(error);
    }
  }

  SerialExecutor executor_;
  static constexpr std::uint32_t kInvalidTrack = 0xffffffffU;
  ::agora::rtc::IRtcEngine* engine_ = nullptr;
  ::agora::media::IMediaEngine* media_ = nullptr;
  ::agora::rtc::track_id_t audio_track_ = kInvalidTrack;
  ::agora::rtc::video_track_id_t video_track_ = kInvalidTrack;
  std::atomic<SdkObserver*> observer_{nullptr};
  bool shutdown_ = false;
};

}  // namespace

std::unique_ptr<Sdk> make_native_sdk() noexcept {
  try {
    return std::make_unique<NativeSdk>();
  } catch (...) {
    return {};
  }
}

}  // namespace voxa::agora
