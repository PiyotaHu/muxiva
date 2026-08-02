#include "voxa/agora_rtc.hpp"

#include "IAgoraMediaEngine.h"
#include "IAgoraRtcEngine.h"

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstdio>
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
        // Agora only activates the per-user before-mixing callback after its
        // output format is configured before joinChannel.
        if ((result = engine_->setPlaybackAudioFrameBeforeMixingParameters(16000, 1)) != 0) {
          return result;
        }
        if ((result = engine_->enableAudio()) != 0) return result;
        ::agora::rtc::AudioTrackConfig audio_config;
        audio_config.enableLocalPlayback = false;
        audio_track_ = media_->createCustomAudioTrack(
            ::agora::rtc::AUDIO_TRACK_DIRECT, audio_config);
        if (audio_track_ == kInvalidTrack) return -1;
        video_track_ = engine_->createCustomVideoTrack();
        if (video_track_ == kInvalidTrack) return -1;
        result = engine_->enableVideo();
        if (result == 0) {
          std::fprintf(stderr,
                       "[VOXA][AGORA][native.initialized] audio=pcm_s16le/16000/mono\n");
        }
        return result;
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
        options.autoSubscribeAudio = true;
        options.autoSubscribeVideo = true;
        options.enableAudioRecordingOrPlayout = true;
        const int result = engine_->joinChannel(token.empty() ? nullptr : token.c_str(),
                                                channel.c_str(), uid, options);
        std::fprintf(stderr,
                     "[VOXA][AGORA][native.join.requested] uid=%u result=%d\n", uid,
                     result);
        return result;
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
      const auto count = received_audio_frames_.fetch_add(1, std::memory_order_relaxed) + 1;
      if (count == 1 || count % 500 == 0) {
        std::fprintf(stderr,
                     "[VOXA][AGORA][audio.received] remote_uid=%u frames=%llu rate_hz=%d "
                     "channels=%d\n",
                     uid, static_cast<unsigned long long>(count), frame.samplesPerSec,
                     frame.channels);
      }
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
    std::fprintf(stderr, "[VOXA][AGORA][connection.state] state=%d reason=%d\n",
                 static_cast<int>(state), static_cast<int>(reason));
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
    std::fprintf(stderr, "[VOXA][AGORA][participant.joined] uid=%u\n", uid);
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_participant_joined(uid);
    }
  }
  void onUserOffline(::agora::rtc::uid_t uid,
                     ::agora::rtc::USER_OFFLINE_REASON_TYPE reason) override {
    std::fprintf(stderr, "[VOXA][AGORA][participant.left] uid=%u reason=%d\n", uid,
                 static_cast<int>(reason));
    if (auto* observer = observer_.load(std::memory_order_acquire)) {
      observer->on_participant_left(uid, static_cast<int>(reason));
    }
  }
  void onError(int error, const char*) override {
    std::fprintf(stderr, "[VOXA][AGORA][native.error] code=%d\n", error);
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
  std::atomic<std::uint64_t> received_audio_frames_{0};
  bool shutdown_ = false;
};

// Agora RTC SDK v4 supports only one IRtcEngine per process. Source and Sink
// Node Packs therefore share this owner through the common provider library.
class SharedEngine final : private SdkObserver {
 public:
  int attach(const std::string& app_id, SdkObserver* observer) noexcept {
    try {
      if (observer == nullptr) return -2;
      std::lock_guard<std::mutex> operation(operation_mutex_);
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        if (std::find(observers_.begin(), observers_.end(), observer) != observers_.end()) return 0;
        if (sdk_) {
          if (app_id_ != app_id) return -2;
          observers_.push_back(observer);
          return 0;
        }
      }
      auto sdk = std::make_shared<NativeSdk>();
      const int result = sdk->initialize(app_id, this);
      if (result != 0) {
        sdk->shutdown();
        return result;
      }
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        if (sdk_) {
          sdk->shutdown();
          return -2;
        }
        sdk_ = std::move(sdk);
        app_id_ = app_id;
        observers_.push_back(observer);
      }
      return 0;
    } catch (...) {
      return -1;
    }
  }

  int join(const std::string& token, const std::string& channel,
           std::uint32_t uid) noexcept {
    try {
      std::lock_guard<std::mutex> operation(operation_mutex_);
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        if (!sdk_) return -7;
        if (joined_) {
          return token_ == token && channel_ == channel && uid_ == uid ? 0 : -2;
        }
        sdk = sdk_;
      }
      const int result = sdk->join(token, channel, uid);
      if (result == 0) {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        token_ = token;
        channel_ = channel;
        uid_ = uid;
        joined_ = true;
      }
      return result;
    } catch (...) {
      return -1;
    }
  }

  int renew_token(const std::string& token) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      if (!sdk) return -7;
      const int result = sdk->renew_token(token);
      if (result == 0) {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        token_ = token;
      }
      return result;
    } catch (...) { return -1; }
  }

  int push_audio(const Pcm16FrameView& frame) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      return sdk ? sdk->push_audio(frame) : -7;
    } catch (...) { return -1; }
  }

  int push_video(const I420FrameView& frame) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      return sdk ? sdk->push_video(frame) : -7;
    } catch (...) { return -1; }
  }

  void detach(SdkObserver* observer) noexcept {
    try {
      std::lock_guard<std::mutex> operation(operation_mutex_);
      std::shared_ptr<NativeSdk> sdk;
      bool leave = false;
      {
        // Holding this lock while erasing also waits for any callback currently
        // invoking this observer, so detach cannot return into freed Node state.
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        observers_.erase(std::remove(observers_.begin(), observers_.end(), observer),
                         observers_.end());
        if (!observers_.empty() || !sdk_) return;
        sdk = std::move(sdk_);
        leave = joined_;
        app_id_.clear();
        token_.clear();
        channel_.clear();
        uid_ = 0;
        joined_ = false;
      }
      // Agora may wait for its callback thread. Never hold the callback-state
      // mutex across vendor calls.
      if (leave) (void)sdk->leave();
      sdk->shutdown();
    } catch (...) {
    }
  }

 private:
  template <typename Action>
  void broadcast(Action action) noexcept {
    try {
      std::lock_guard<std::recursive_mutex> lock(mutex_);
      for (auto* observer : observers_) {
        if (observer != nullptr) action(*observer);
      }
    } catch (...) {
    }
  }

  void on_connection_state(ConnectionState state, int reason) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_connection_state(state, reason); });
  }
  void on_rejoined(std::uint32_t uid, int elapsed) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_rejoined(uid, elapsed); });
  }
  void on_connection_lost() noexcept override {
    broadcast([](SdkObserver& value) { value.on_connection_lost(); });
  }
  void on_token_expiring() noexcept override {
    broadcast([](SdkObserver& value) { value.on_token_expiring(); });
  }
  void on_token_required() noexcept override {
    broadcast([](SdkObserver& value) { value.on_token_required(); });
  }
  void on_network_quality(std::uint32_t uid, int tx, int rx) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_network_quality(uid, tx, rx); });
  }
  void on_rtc_stats(const RtcStatsSnapshot& stats) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_rtc_stats(stats); });
  }
  void on_participant_joined(std::uint32_t uid) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_participant_joined(uid); });
  }
  void on_participant_left(std::uint32_t uid, int reason) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_participant_left(uid, reason); });
  }
  void on_error(int code) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_error(code); });
  }
  void on_audio_frame(const Pcm16FrameView& frame) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_audio_frame(frame); });
  }
  void on_video_frame(const I420FrameView& frame) noexcept override {
    broadcast([&](SdkObserver& value) { value.on_video_frame(frame); });
  }

  std::recursive_mutex mutex_;
  std::mutex operation_mutex_;
  std::shared_ptr<NativeSdk> sdk_;
  std::vector<SdkObserver*> observers_;
  std::string app_id_;
  std::string token_;
  std::string channel_;
  std::uint32_t uid_ = 0;
  bool joined_ = false;
};

SharedEngine& shared_engine() noexcept {
  // Deliberately process-lived: Agora may retain internal threads until process
  // shutdown, and IRtcEngine is a process singleton by contract.
  static auto* value = new SharedEngine();
  return *value;
}

class SharedSdk final : public Sdk {
 public:
  ~SharedSdk() override { shutdown(); }

  int initialize(const std::string& app_id, SdkObserver* observer) noexcept override {
    if (attached_) return -2;
    const int result = shared_engine().attach(app_id, observer);
    if (result == 0) {
      observer_ = observer;
      attached_ = true;
    }
    return result;
  }
  int join(const std::string& token, const std::string& channel,
           std::uint32_t uid) noexcept override {
    return attached_ ? shared_engine().join(token, channel, uid) : -7;
  }
  int leave() noexcept override { return attached_ ? 0 : -7; }
  int renew_token(const std::string& token) noexcept override {
    return attached_ ? shared_engine().renew_token(token) : -7;
  }
  int push_audio(const Pcm16FrameView& frame) noexcept override {
    return attached_ ? shared_engine().push_audio(frame) : -7;
  }
  int push_video(const I420FrameView& frame) noexcept override {
    return attached_ ? shared_engine().push_video(frame) : -7;
  }
  void shutdown() noexcept override {
    if (!attached_) return;
    shared_engine().detach(observer_);
    observer_ = nullptr;
    attached_ = false;
  }

 private:
  SdkObserver* observer_ = nullptr;
  bool attached_ = false;
};

}  // namespace

std::unique_ptr<Sdk> make_native_sdk() noexcept {
  try {
    return std::make_unique<SharedSdk>();
  } catch (...) {
    return {};
  }
}

}  // namespace voxa::agora
