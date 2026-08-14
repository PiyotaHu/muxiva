#include "muxiva/agora_rtc.hpp"

#include "IAgoraService.h"
#include "NGIAgoraAudioTrack.h"
#include "NGIAgoraLocalUser.h"
#include "NGIAgoraMediaNode.h"
#include "NGIAgoraMediaNodeFactory.h"
#include "NGIAgoraRtcConnection.h"
#include "NGIAgoraVideoTrack.h"

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <functional>
#include <future>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <utility>
#include <vector>

// Linux implementation of the vendor-neutral `muxiva::agora::Sdk` contract.
//
// The macOS implementation (agora_native_sdk.cc) is built on the Agora *client*
// SDK (`IAgoraRtcEngine`), which Agora does not publish for Linux. This file is
// the parallel Linux implementation built on the Agora *Server Gateway* SDK
// (formerly RTSA), whose public surface is `IAgoraService` + `IRtcConnection` +
// `ILocalUser` + media tracks. It mirrors the macOS file's structure and
// semantics: a serial SDK control thread, a process-level shared engine, and a
// per-node `SharedSdk` facade. The surrounding architecture (`SharedSession`,
// `RtcAdapter`, and every graph Node) is unchanged.

namespace muxiva::agora {
namespace {

class SerialExecutor final {
 public:
  SerialExecutor() : worker_([this] { run(); }) {}
  SerialExecutor(const SerialExecutor &) = delete;
  SerialExecutor &operator=(const SerialExecutor &) = delete;
  ~SerialExecutor() { stop(); }

  int call(std::function<int()> action) noexcept {
    try {
      auto task =
          std::make_shared<std::packaged_task<int()>>(std::move(action));
      auto future = task->get_future();
      {
        std::lock_guard<std::mutex> lock(mutex_);
        if (stopping_)
          return -7;
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
      if (stopping_)
        return;
      stopping_ = true;
    }
    cv_.notify_all();
    if (worker_.joinable())
      worker_.join();
  }

 private:
  void run() noexcept {
    for (;;) {
      std::function<void()> action;
      {
        std::unique_lock<std::mutex> lock(mutex_);
        cv_.wait(lock, [&] { return stopping_ || !queue_.empty(); });
        if (queue_.empty() && stopping_)
          return;
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

// The Server Gateway SDK represents every user id as a decimal string. Muxiva's
// graph contract uses numeric uids (for example the bot uid `2001` and the
// browser uid `1001`), so the two representations are converted at the SDK
// boundary.
std::string uid_to_string(std::uint32_t uid) { return std::to_string(uid); }

std::uint32_t uid_from_string(const char *uid) noexcept {
  if (uid == nullptr || *uid == '\0')
    return 0;
  return static_cast<std::uint32_t>(std::strtoul(uid, nullptr, 10));
}

class NativeSdk final : public Sdk,
                        private ::agora::rtc::IRtcConnectionObserver,
                        private ::agora::rtc::ILocalUserObserver,
                        private ::agora::media::IAudioFrameObserverBase,
                        private ::agora::rtc::IVideoFrameObserver2 {
 public:
  ~NativeSdk() override { shutdown(); }

  int initialize(const std::string &app_id,
                 SdkObserver *observer) noexcept override {
    try {
      return executor_.call([this, app_id, observer] {
        if (service_ != nullptr || observer == nullptr)
          return -2;
        observer_.store(observer, std::memory_order_release);

        service_ = ::createAgoraService();
        if (service_ == nullptr)
          return -1;

        ::agora::base::AgoraServiceConfiguration config;
        config.appId = app_id.c_str();
        config.enableAudioProcessor = true;
        config.enableAudioDevice = false;  // server: no capture/playout device
        config.enableVideo = true;
        config.useStringUid = true;
        config.channelProfile = ::agora::CHANNEL_PROFILE_COMMUNICATION;
        int result = service_->initialize(config);
        if (result != 0)
          return result;

        ::agora::rtc::RtcConnectionConfiguration connection_config;
        connection_config.autoSubscribeAudio = true;
        connection_config.autoSubscribeVideo = true;
        connection_config.enableAudioRecordingOrPlayout = true;
        connection_config.clientRoleType = ::agora::rtc::CLIENT_ROLE_BROADCASTER;
        connection_config.channelProfile =
            ::agora::CHANNEL_PROFILE_COMMUNICATION;
        connection_ = service_->createRtcConnection(connection_config);
        if (!connection_)
          return -1;
        connection_->registerObserver(this);

        local_user_ = connection_->getLocalUser();
        if (local_user_ == nullptr)
          return -1;
        local_user_->registerLocalUserObserver(this);

        factory_ = service_->createMediaNodeFactory();
        if (!factory_)
          return -1;

        audio_sender_ = factory_->createAudioPcmDataSender();
        if (!audio_sender_)
          return -1;
        audio_track_ = service_->createCustomAudioTrack(audio_sender_);
        if (!audio_track_)
          return -1;

        video_sender_ = factory_->createVideoFrameSender();
        if (!video_sender_)
          return -1;
        video_track_ = service_->createCustomVideoTrack(video_sender_);
        if (!video_track_)
          return -1;

        if ((result = connection_->createDataStream(&data_stream_id_, true, true,
                                                    true)) != 0) {
          return result;
        }

        // Receive each remote user's pre-mixing PCM at 16 kHz mono, mirroring
        // the macOS per-user before-mixing callback.
        if ((result = local_user_->setPlaybackAudioFrameBeforeMixingParameters(
                 1, 16000)) != 0) {
          return result;
        }
        if ((result = local_user_->registerAudioFrameObserver(this)) != 0)
          return result;
        if ((result = local_user_->registerVideoFrameObserver(this)) != 0)
          return result;

        std::fprintf(
            stderr,
            "[MUXIVA][AGORA][native.initialized] audio=pcm_s16le/16000/mono "
            "transport=server-gateway\n");
        return 0;
      });
    } catch (...) {
      return -1;
    }
  }

  int join(const std::string &token, const std::string &channel,
           std::uint32_t uid) noexcept override {
    try {
      return executor_.call([this, token, channel, uid] {
        if (connection_ == nullptr || local_user_ == nullptr)
          return -7;
        const std::string uid_string = uid_to_string(uid);
        const int result =
            connection_->connect(token.empty() ? nullptr : token.c_str(),
                                 channel.c_str(), uid_string.c_str());
        std::fprintf(stderr,
                     "[MUXIVA][AGORA][native.join.requested] uid=%u result=%d\n",
                     uid, result);
        if (result == 0) {
          (void)local_user_->publishAudio(audio_track_);
          (void)local_user_->publishVideo(video_track_);
        }
        return result;
      });
    } catch (...) {
      return -1;
    }
  }

  int leave() noexcept override {
    return executor_.call([this] {
      if (connection_ == nullptr)
        return 0;
      if (local_user_ != nullptr) {
        (void)local_user_->unpublishAudio(audio_track_);
        (void)local_user_->unpublishVideo(video_track_);
      }
      return connection_->disconnect();
    });
  }

  int renew_token(const std::string &token) noexcept override {
    try {
      return executor_.call([this, token] {
        return connection_ == nullptr ? -7
                                      : connection_->renewToken(token.c_str());
      });
    } catch (...) {
      return -1;
    }
  }

  int push_audio(const Pcm16FrameView &value) noexcept override {
    try {
      std::vector<std::uint8_t> bytes(value.data, value.data + value.size);
      return executor_.call([this, bytes = std::move(bytes), value]() mutable {
        if (audio_sender_ == nullptr || value.sample_rate_hz != 48000 ||
            value.channels != 1) {
          return -2;
        }
        return audio_sender_->sendAudioPcmData(
            bytes.data(), static_cast<std::uint32_t>(value.timestamp_ms), 0,
            static_cast<std::size_t>(value.samples_per_channel),
            ::agora::rtc::TWO_BYTES_PER_SAMPLE,
            static_cast<std::size_t>(value.channels), value.sample_rate_hz);
      });
    } catch (...) {
      return -1;
    }
  }

  int push_video(const I420FrameView &value) noexcept override {
    try {
      const auto pixels = static_cast<std::size_t>(value.width) * value.height;
      std::vector<std::uint8_t> bytes(pixels + pixels / 2);
      for (std::uint32_t row = 0; row < value.height; ++row) {
        std::copy_n(value.y + static_cast<std::size_t>(row) * value.y_stride,
                    value.width,
                    bytes.data() + static_cast<std::size_t>(row) * value.width);
      }
      auto *u = bytes.data() + pixels;
      auto *v = u + pixels / 4;
      for (std::uint32_t row = 0; row < value.height / 2; ++row) {
        std::copy_n(value.u + static_cast<std::size_t>(row) * value.u_stride,
                    value.width / 2,
                    u + static_cast<std::size_t>(row) * value.width / 2);
        std::copy_n(value.v + static_cast<std::size_t>(row) * value.v_stride,
                    value.width / 2,
                    v + static_cast<std::size_t>(row) * value.width / 2);
      }
      return executor_.call([this, bytes = std::move(bytes), value]() mutable {
        if (video_sender_ == nullptr)
          return -7;
        ::agora::media::base::ExternalVideoFrame frame;
        frame.type =
            ::agora::media::base::ExternalVideoFrame::VIDEO_BUFFER_RAW_DATA;
        frame.format = ::agora::media::base::VIDEO_PIXEL_I420;
        frame.buffer = bytes.data();
        frame.stride = static_cast<int>(value.width);
        frame.height = static_cast<int>(value.height);
        frame.timestamp = value.timestamp_ms;
        return video_sender_->sendVideoFrame(frame);
      });
    } catch (...) {
      return -1;
    }
  }

  int push_data(const DataMessageView &value) noexcept override {
    try {
      if (value.data == nullptr || value.size == 0 || value.size > 1024 ||
          data_stream_id_ < 0)
        return -2;
      std::string bytes(reinterpret_cast<const char *>(value.data), value.size);
      return executor_.call([this, bytes = std::move(bytes)] {
        return connection_ == nullptr
                   ? -7
                   : connection_->sendStreamMessage(data_stream_id_,
                                                    bytes.data(), bytes.size());
      });
    } catch (...) {
      return -1;
    }
  }

  void shutdown() noexcept override {
    if (shutdown_)
      return;
    shutdown_ = true;
    (void)executor_.call([this] {
      observer_.store(nullptr, std::memory_order_release);
      if (local_user_ != nullptr) {
        (void)local_user_->unregisterAudioFrameObserver(this);
        (void)local_user_->unregisterVideoFrameObserver(this);
        (void)local_user_->unregisterLocalUserObserver(this);
        local_user_ = nullptr;
      }
      audio_track_.reset();
      video_track_.reset();
      audio_sender_.reset();
      video_sender_.reset();
      factory_.reset();
      if (connection_) {
        (void)connection_->unregisterObserver(this);
        connection_.reset();
      }
      if (service_ != nullptr) {
        (void)service_->release();
        service_ = nullptr;
      }
      return 0;
    });
    executor_.stop();
  }

 private:
  using AudioFrame = ::agora::media::IAudioFrameObserverBase::AudioFrame;
  using AudioParams = ::agora::media::IAudioFrameObserverBase::AudioParams;

  // --- IRtcConnectionObserver -------------------------------------------------
  void onConnected(const ::agora::rtc::TConnectionInfo &info,
                   ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    forward_connection_state(static_cast<ConnectionState>(info.state),
                             static_cast<int>(reason));
  }
  void onDisconnected(const ::agora::rtc::TConnectionInfo &info,
                      ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    forward_connection_state(static_cast<ConnectionState>(info.state),
                             static_cast<int>(reason));
  }
  void onConnecting(const ::agora::rtc::TConnectionInfo &info,
                    ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    forward_connection_state(static_cast<ConnectionState>(info.state),
                             static_cast<int>(reason));
  }
  void onReconnecting(const ::agora::rtc::TConnectionInfo &info,
                      ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    forward_connection_state(static_cast<ConnectionState>(info.state),
                             static_cast<int>(reason));
  }
  void onReconnected(const ::agora::rtc::TConnectionInfo &info,
                     ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    forward_connection_state(static_cast<ConnectionState>(info.state),
                             static_cast<int>(reason));
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_rejoined(static_cast<std::uint32_t>(info.internalUid), 0);
    }
  }
  void onConnectionLost(
      const ::agora::rtc::TConnectionInfo &) override {
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_connection_lost();
    }
  }
  void onLastmileQuality(const ::agora::rtc::QUALITY_TYPE) override {}
  void onLastmileProbeResult(
      const ::agora::rtc::LastmileProbeResult &) override {}
  void onTokenPrivilegeWillExpire(const char *) override {
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_token_expiring();
    }
  }
  void onTokenPrivilegeDidExpire() override {
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_token_required();
    }
  }
  void onConnectionFailure(
      const ::agora::rtc::TConnectionInfo &info,
      ::agora::rtc::CONNECTION_CHANGED_REASON_TYPE reason) override {
    forward_connection_state(static_cast<ConnectionState>(info.state),
                             static_cast<int>(reason));
  }
  void onUserJoined(::agora::user_id_t userId) override {
    const auto uid = uid_from_string(userId);
    std::fprintf(stderr, "[MUXIVA][AGORA][participant.joined] uid=%u\n", uid);
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_participant_joined(uid);
    }
  }
  void onUserLeft(::agora::user_id_t userId,
                  ::agora::rtc::USER_OFFLINE_REASON_TYPE reason) override {
    const auto uid = uid_from_string(userId);
    std::fprintf(stderr, "[MUXIVA][AGORA][participant.left] uid=%u reason=%d\n",
                 uid, static_cast<int>(reason));
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_participant_left(uid, static_cast<int>(reason));
    }
  }
  void onTransportStats(const ::agora::rtc::RtcStats &stats) override {
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_rtc_stats({stats.duration, stats.txBytes, stats.rxBytes,
                              stats.userCount, stats.lastmileDelay});
    }
  }
  void onChannelMediaRelayStateChanged(int, int) override {}
  void onUserNetworkQuality(::agora::user_id_t userId,
                            ::agora::rtc::QUALITY_TYPE txQuality,
                            ::agora::rtc::QUALITY_TYPE rxQuality) override {
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_network_quality(uid_from_string(userId),
                                   static_cast<int>(txQuality),
                                   static_cast<int>(rxQuality));
    }
  }
  void onError(::agora::ERROR_CODE_TYPE error, const char *) override {
    std::fprintf(stderr, "[MUXIVA][AGORA][native.error] code=%d\n",
                 static_cast<int>(error));
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_error(static_cast<int>(error));
    }
  }

  void forward_connection_state(ConnectionState state, int reason) noexcept {
    std::fprintf(stderr, "[MUXIVA][AGORA][connection.state] state=%d reason=%d\n",
                 static_cast<int>(state), reason);
    if (auto *observer = observer_.load(std::memory_order_acquire)) {
      observer->on_connection_state(state, reason);
    }
  }

  // --- ILocalUserObserver -----------------------------------------------------
  void onStreamMessage(::agora::user_id_t userId, int streamId,
                       const char *data, size_t length) override {
    auto *observer = observer_.load(std::memory_order_acquire);
    if (observer != nullptr && data != nullptr && length > 0 && length <= 1024) {
      observer->on_data_message(
          {reinterpret_cast<const std::uint8_t *>(data), length,
           uid_from_string(userId), streamId, 0});
    }
  }
  void onAudioTrackPublishStart(
      ::agora::agora_refptr<::agora::rtc::ILocalAudioTrack>) override {}
  void onAudioTrackPublishSuccess(
      ::agora::agora_refptr<::agora::rtc::ILocalAudioTrack>) override {}
  void onAudioTrackUnpublished(
      ::agora::agora_refptr<::agora::rtc::ILocalAudioTrack>) override {}
  void onAudioTrackPublicationFailure(
      ::agora::agora_refptr<::agora::rtc::ILocalAudioTrack>,
      ::agora::ERROR_CODE_TYPE) override {}
  void onLocalAudioTrackStatistics(
      const ::agora::rtc::LocalAudioStats &) override {}
  void onRemoteAudioTrackStatistics(
      ::agora::agora_refptr<::agora::rtc::IRemoteAudioTrack>,
      const ::agora::rtc::RemoteAudioTrackStats &) override {}
  void onUserAudioTrackSubscribed(
      ::agora::user_id_t,
      ::agora::agora_refptr<::agora::rtc::IRemoteAudioTrack>) override {}
  void onUserAudioTrackStateChanged(
      ::agora::user_id_t,
      ::agora::agora_refptr<::agora::rtc::IRemoteAudioTrack>,
      ::agora::rtc::REMOTE_AUDIO_STATE, ::agora::rtc::REMOTE_AUDIO_STATE_REASON,
      int) override {}
  void onVideoTrackPublishStart(
      ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack>) override {}
  void onVideoTrackPublishSuccess(
      ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack>) override {}
  void onVideoTrackPublicationFailure(
      ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack>,
      ::agora::ERROR_CODE_TYPE) override {}
  void onVideoTrackUnpublished(
      ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack>) override {}
  void onLocalVideoTrackStateChanged(
      ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack>,
      ::agora::rtc::LOCAL_VIDEO_STREAM_STATE,
      ::agora::rtc::LOCAL_VIDEO_STREAM_REASON) override {}
  void onLocalVideoTrackStatistics(
      ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack>,
      const ::agora::rtc::LocalVideoTrackStats &) override {}
  void onUserVideoTrackSubscribed(
      ::agora::user_id_t, const ::agora::rtc::VideoTrackInfo &,
      ::agora::agora_refptr<::agora::rtc::IRemoteVideoTrack>) override {}
  void onUserVideoTrackStateChanged(
      ::agora::user_id_t,
      ::agora::agora_refptr<::agora::rtc::IRemoteVideoTrack>,
      ::agora::rtc::REMOTE_VIDEO_STATE, ::agora::rtc::REMOTE_VIDEO_STATE_REASON,
      int) override {}
  void onFirstRemoteVideoFrameRendered(::agora::user_id_t, int, int,
                                       int) override {}
  void onRemoteVideoTrackStatistics(
      ::agora::agora_refptr<::agora::rtc::IRemoteVideoTrack>,
      const ::agora::rtc::RemoteVideoTrackStats &) override {}
  void onAudioVolumeIndication(
      const ::agora::rtc::AudioVolumeInformation *, unsigned int, int) override {
  }
  void onActiveSpeaker(::agora::user_id_t) override {}
  void onAudioSubscribeStateChanged(const char *, ::agora::user_id_t,
                                    ::agora::rtc::STREAM_SUBSCRIBE_STATE,
                                    ::agora::rtc::STREAM_SUBSCRIBE_STATE,
                                    int) override {}
  void onVideoSubscribeStateChanged(const char *, ::agora::user_id_t,
                                    ::agora::rtc::STREAM_SUBSCRIBE_STATE,
                                    ::agora::rtc::STREAM_SUBSCRIBE_STATE,
                                    int) override {}
  void onAudioPublishStateChanged(const char *, ::agora::rtc::STREAM_PUBLISH_STATE,
                                  ::agora::rtc::STREAM_PUBLISH_STATE,
                                  int) override {}
  void onVideoPublishStateChanged(const char *, ::agora::rtc::STREAM_PUBLISH_STATE,
                                  ::agora::rtc::STREAM_PUBLISH_STATE,
                                  int) override {}
  void onFirstRemoteAudioFrame(::agora::user_id_t, int) override {}
  void onFirstRemoteAudioDecoded(::agora::user_id_t, int) override {}
  void onFirstRemoteVideoFrame(::agora::user_id_t, int, int, int) override {
  }
  void onFirstRemoteVideoDecoded(::agora::user_id_t, int, int, int) override {
  }
  void onVideoSizeChanged(::agora::user_id_t, int, int, int) override {}

  // --- IAudioFrameObserverBase ------------------------------------------------
  bool onRecordAudioFrame(const char *, AudioFrame &) override { return true; }
  bool onPlaybackAudioFrame(const char *, AudioFrame &frame) override {
    // Silence the final mixed playout so a server without an audio device never
    // reflects the bot's own output back into the room. Mirrors macOS behavior.
    if (frame.buffer != nullptr && frame.samplesPerChannel > 0 &&
        frame.channels > 0 && frame.bytesPerSample > 0) {
      const auto size = static_cast<std::size_t>(frame.samplesPerChannel) *
                        static_cast<std::size_t>(frame.channels) *
                        static_cast<std::size_t>(frame.bytesPerSample);
      std::memset(frame.buffer, 0, size);
    }
    return true;
  }
  bool onMixedAudioFrame(const char *, AudioFrame &) override { return true; }
  bool onEarMonitoringAudioFrame(AudioFrame &) override { return true; }
  bool onPlaybackAudioFrameBeforeMixing(const char *,
                                        ::agora::media::base::user_id_t userId,
                                        AudioFrame &frame) override {
    auto *observer = observer_.load(std::memory_order_acquire);
    if (observer != nullptr && frame.buffer != nullptr &&
        frame.samplesPerChannel > 0 && frame.channels > 0 &&
        frame.bytesPerSample == ::agora::rtc::TWO_BYTES_PER_SAMPLE) {
      const auto size = static_cast<std::size_t>(frame.samplesPerChannel) *
                        frame.channels * 2;
      observer->on_audio_frame(
          {static_cast<const std::uint8_t *>(frame.buffer), size,
           static_cast<std::uint32_t>(frame.samplesPerSec),
           static_cast<std::uint16_t>(frame.channels),
           static_cast<std::uint64_t>(frame.samplesPerChannel),
           frame.renderTimeMs, uid_from_string(userId)});
      const auto count =
          received_audio_frames_.fetch_add(1, std::memory_order_relaxed) + 1;
      if (count == 1 || count % 500 == 0) {
        std::fprintf(stderr,
                     "[MUXIVA][AGORA][audio.received] remote_uid=%u frames=%llu "
                     "rate_hz=%d channels=%d\n",
                     uid_from_string(userId),
                     static_cast<unsigned long long>(count), frame.samplesPerSec,
                     frame.channels);
      }
    }
    return true;
  }
  int getObservedAudioFramePosition() override {
    return AUDIO_FRAME_POSITION_BEFORE_MIXING | AUDIO_FRAME_POSITION_PLAYBACK;
  }
  AudioParams getPlaybackAudioParams() override { return {}; }
  AudioParams getRecordAudioParams() override { return {}; }
  AudioParams getMixedAudioParams() override { return {}; }
  AudioParams getEarMonitoringAudioParams() override { return {}; }

  // --- IVideoFrameObserver2 ---------------------------------------------------
  void onFrame(const char *, ::agora::user_id_t remoteUid,
               const ::agora::media::base::VideoFrame *frame) override {
    auto *observer = observer_.load(std::memory_order_acquire);
    if (observer != nullptr && frame != nullptr &&
        frame->type == ::agora::media::base::VIDEO_PIXEL_I420 &&
        frame->width > 0 && frame->height > 0) {
      observer->on_video_frame(
          {frame->yBuffer, frame->uBuffer, frame->vBuffer,
           static_cast<std::size_t>(frame->yStride),
           static_cast<std::size_t>(frame->uStride),
           static_cast<std::size_t>(frame->vStride),
           static_cast<std::uint32_t>(frame->width),
           static_cast<std::uint32_t>(frame->height), frame->renderTimeMs,
           uid_from_string(remoteUid)});
    }
  }

  SerialExecutor executor_;
  ::agora::base::IAgoraService *service_ = nullptr;
  ::agora::agora_refptr<::agora::rtc::IRtcConnection> connection_;
  ::agora::rtc::ILocalUser *local_user_ = nullptr;
  ::agora::agora_refptr<::agora::rtc::IMediaNodeFactory> factory_;
  ::agora::agora_refptr<::agora::rtc::IAudioPcmDataSender> audio_sender_;
  ::agora::agora_refptr<::agora::rtc::ILocalAudioTrack> audio_track_;
  ::agora::agora_refptr<::agora::rtc::IVideoFrameSender> video_sender_;
  ::agora::agora_refptr<::agora::rtc::ILocalVideoTrack> video_track_;
  int data_stream_id_ = -1;
  std::atomic<SdkObserver *> observer_{nullptr};
  std::atomic<std::uint64_t> received_audio_frames_{0};
  bool shutdown_ = false;
};

// The Server Gateway SDK allows one connection per `IAgoraService`, and the
// Muxiva source/sink Node Packs share one process-level session. Mirrors the
// macOS `SharedEngine` owner exactly.
class SharedEngine final : private SdkObserver {
 public:
  int attach(const std::string &app_id, SdkObserver *observer) noexcept {
    try {
      if (observer == nullptr)
        return -2;
      std::lock_guard<std::mutex> operation(operation_mutex_);
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        if (std::find(observers_.begin(), observers_.end(), observer) !=
            observers_.end())
          return 0;
        if (sdk_) {
          if (app_id_ != app_id)
            return -2;
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

  int join(const std::string &token, const std::string &channel,
           std::uint32_t uid) noexcept {
    try {
      std::lock_guard<std::mutex> operation(operation_mutex_);
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        if (!sdk_)
          return -7;
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

  int renew_token(const std::string &token) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      if (!sdk)
        return -7;
      const int result = sdk->renew_token(token);
      if (result == 0) {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        token_ = token;
      }
      return result;
    } catch (...) {
      return -1;
    }
  }

  int push_audio(const Pcm16FrameView &frame) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      return sdk ? sdk->push_audio(frame) : -7;
    } catch (...) {
      return -1;
    }
  }

  int push_video(const I420FrameView &frame) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      return sdk ? sdk->push_video(frame) : -7;
    } catch (...) {
      return -1;
    }
  }

  int push_data(const DataMessageView &message) noexcept {
    try {
      std::shared_ptr<NativeSdk> sdk;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        sdk = sdk_;
      }
      return sdk ? sdk->push_data(message) : -7;
    } catch (...) {
      return -1;
    }
  }

  void detach(SdkObserver *observer) noexcept {
    try {
      std::lock_guard<std::mutex> operation(operation_mutex_);
      std::shared_ptr<NativeSdk> sdk;
      bool leave = false;
      {
        std::lock_guard<std::recursive_mutex> lock(mutex_);
        observers_.erase(
            std::remove(observers_.begin(), observers_.end(), observer),
            observers_.end());
        if (!observers_.empty() || !sdk_)
          return;
        sdk = std::move(sdk_);
        leave = joined_;
        app_id_.clear();
        token_.clear();
        channel_.clear();
        uid_ = 0;
        joined_ = false;
      }
      if (leave)
        (void)sdk->leave();
      sdk->shutdown();
    } catch (...) {
    }
  }

 private:
  template <typename Action> void broadcast(Action action) noexcept {
    try {
      std::lock_guard<std::recursive_mutex> lock(mutex_);
      for (auto *observer : observers_) {
        if (observer != nullptr)
          action(*observer);
      }
    } catch (...) {
    }
  }

  void on_connection_state(ConnectionState state, int reason) noexcept override {
    broadcast(
        [&](SdkObserver &value) { value.on_connection_state(state, reason); });
  }
  void on_rejoined(std::uint32_t uid, int elapsed) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_rejoined(uid, elapsed); });
  }
  void on_connection_lost() noexcept override {
    broadcast([](SdkObserver &value) { value.on_connection_lost(); });
  }
  void on_token_expiring() noexcept override {
    broadcast([](SdkObserver &value) { value.on_token_expiring(); });
  }
  void on_token_required() noexcept override {
    broadcast([](SdkObserver &value) { value.on_token_required(); });
  }
  void on_network_quality(std::uint32_t uid, int tx, int rx) noexcept override {
    broadcast(
        [&](SdkObserver &value) { value.on_network_quality(uid, tx, rx); });
  }
  void on_rtc_stats(const RtcStatsSnapshot &stats) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_rtc_stats(stats); });
  }
  void on_participant_joined(std::uint32_t uid) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_participant_joined(uid); });
  }
  void on_participant_left(std::uint32_t uid, int reason) noexcept override {
    broadcast(
        [&](SdkObserver &value) { value.on_participant_left(uid, reason); });
  }
  void on_error(int code) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_error(code); });
  }
  void on_audio_frame(const Pcm16FrameView &frame) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_audio_frame(frame); });
  }
  void on_video_frame(const I420FrameView &frame) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_video_frame(frame); });
  }
  void on_data_message(const DataMessageView &message) noexcept override {
    broadcast([&](SdkObserver &value) { value.on_data_message(message); });
  }

  std::recursive_mutex mutex_;
  std::mutex operation_mutex_;
  std::shared_ptr<NativeSdk> sdk_;
  std::vector<SdkObserver *> observers_;
  std::string app_id_;
  std::string token_;
  std::string channel_;
  std::uint32_t uid_ = 0;
  bool joined_ = false;
};

SharedEngine &shared_engine() noexcept {
  static auto *value = new SharedEngine();
  return *value;
}

class SharedSdk final : public Sdk {
 public:
  ~SharedSdk() override { shutdown(); }

  int initialize(const std::string &app_id,
                 SdkObserver *observer) noexcept override {
    if (attached_)
      return -2;
    const int result = shared_engine().attach(app_id, observer);
    if (result == 0) {
      observer_ = observer;
      attached_ = true;
    }
    return result;
  }
  int join(const std::string &token, const std::string &channel,
           std::uint32_t uid) noexcept override {
    return attached_ ? shared_engine().join(token, channel, uid) : -7;
  }
  int leave() noexcept override { return attached_ ? 0 : -7; }
  int renew_token(const std::string &token) noexcept override {
    return attached_ ? shared_engine().renew_token(token) : -7;
  }
  int push_audio(const Pcm16FrameView &frame) noexcept override {
    return attached_ ? shared_engine().push_audio(frame) : -7;
  }
  int push_video(const I420FrameView &frame) noexcept override {
    return attached_ ? shared_engine().push_video(frame) : -7;
  }
  int push_data(const DataMessageView &message) noexcept override {
    return attached_ ? shared_engine().push_data(message) : -7;
  }
  void shutdown() noexcept override {
    if (!attached_)
      return;
    shared_engine().detach(observer_);
    observer_ = nullptr;
    attached_ = false;
  }

 private:
  SdkObserver *observer_ = nullptr;
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

}  // namespace muxiva::agora
