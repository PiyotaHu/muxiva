#include "muxiva/agora_rtc.hpp"

#include <cstdio>
#include <deque>
#include <mutex>
#include <stdexcept>
#include <utility>

namespace muxiva::agora {
namespace {
std::mutex global_mutex;
std::weak_ptr<SharedSession> global_session;
constexpr std::size_t kMaximumAudioFrames = 512;
constexpr std::size_t kMaximumDataMessages = 256;
}  // namespace

struct SharedSession::Impl final {
  std::unique_ptr<Sdk> sdk;
  mutable std::mutex mutex;
  std::deque<OwnedPcm16Frame> audio;
  std::deque<OwnedDataMessage> data;
  std::uint64_t audio_received_total = 0;
  std::uint64_t audio_dropped_total = 0;
  std::uint64_t audio_queued_duration_ns = 0;
};

std::uint64_t audio_duration_ns(const OwnedPcm16Frame& frame) noexcept {
  if (frame.sample_rate_hz == 0) return 0;
  return frame.samples_per_channel * 1000000000ULL / frame.sample_rate_hz;
}

std::shared_ptr<SharedSession> SharedSession::acquire(
    const std::string& app_id, const std::string& token,
    const std::string& channel, std::uint32_t bot_uid,
    std::uint32_t allowed_remote_uid) {
  std::lock_guard<std::mutex> lock(global_mutex);
  if (auto current = global_session.lock()) {
    if (current->app_id_ != app_id || current->token_ != token ||
        current->channel_ != channel || current->bot_uid_ != bot_uid ||
        current->allowed_remote_uid_ != allowed_remote_uid) {
      throw std::runtime_error(
          "one Runtime process supports one Agora session; all Agora Nodes must share its credentials and identities");
    }
    return current;
  }
  auto session = std::shared_ptr<SharedSession>(new SharedSession(
      app_id, token, channel, bot_uid, allowed_remote_uid));
  global_session = session;
  return session;
}

SharedSession::SharedSession(std::string app_id, std::string token,
                             std::string channel, std::uint32_t bot_uid,
                             std::uint32_t allowed_remote_uid)
    : impl_(std::make_unique<Impl>()), app_id_(std::move(app_id)),
      token_(std::move(token)), channel_(std::move(channel)),
      bot_uid_(bot_uid), allowed_remote_uid_(allowed_remote_uid) {
  impl_->sdk = make_native_sdk();
  if (!impl_->sdk)
    throw std::runtime_error("Agora Native SDK is not enabled in this build");
  if (impl_->sdk->initialize(app_id_, this) != 0 ||
      impl_->sdk->join(token_, channel_, bot_uid_) != 0) {
    impl_->sdk->shutdown();
    throw std::runtime_error("Agora C++ SDK failed to join the configured room");
  }
  std::fprintf(stderr,
               "[MUXIVA][AGORA][session.shared] channel=%s bot_uid=%u participant_uid=%u\n",
               channel_.c_str(), bot_uid_, allowed_remote_uid_);
}

SharedSession::~SharedSession() noexcept {
  if (impl_ && impl_->sdk) {
    impl_->sdk->leave();
    impl_->sdk->shutdown();
  }
}

bool SharedSession::try_pop_audio(OwnedPcm16Frame& frame) noexcept {
  std::lock_guard<std::mutex> lock(impl_->mutex);
  if (impl_->audio.empty()) return false;
  frame = std::move(impl_->audio.front());
  impl_->audio.pop_front();
  impl_->audio_queued_duration_ns -=
      std::min(impl_->audio_queued_duration_ns, audio_duration_ns(frame));
  return true;
}

AudioIngressStats SharedSession::audio_ingress_stats() const noexcept {
  std::lock_guard<std::mutex> lock(impl_->mutex);
  return {impl_->audio_received_total, impl_->audio_dropped_total,
          impl_->audio.size(), impl_->audio_queued_duration_ns};
}

bool SharedSession::try_pop_data(OwnedDataMessage& message) noexcept {
  std::lock_guard<std::mutex> lock(impl_->mutex);
  if (impl_->data.empty()) return false;
  message = std::move(impl_->data.front());
  impl_->data.pop_front();
  return true;
}

int SharedSession::send_audio(const Pcm16FrameView& frame) noexcept {
  return impl_->sdk ? impl_->sdk->push_audio(frame) : -7;
}

int SharedSession::send_data(const std::uint8_t* data,
                             std::size_t size) noexcept {
  if (data == nullptr || size == 0 || size > 1024) return -2;
  return impl_->sdk ? impl_->sdk->push_data({data, size, 0, 0, 0}) : -7;
}

void SharedSession::on_audio_frame(const Pcm16FrameView& frame) noexcept {
  try {
    if (frame.remote_uid != allowed_remote_uid_ || frame.data == nullptr ||
        frame.size == 0 || frame.size > 256U * 1024U) return;
    OwnedPcm16Frame owned{{frame.data, frame.data + frame.size},
                          frame.sample_rate_hz, frame.channels,
                          frame.samples_per_channel, frame.timestamp_ms,
                          frame.remote_uid};
    std::lock_guard<std::mutex> lock(impl_->mutex);
    ++impl_->audio_received_total;
    if (impl_->audio.size() == kMaximumAudioFrames) {
      impl_->audio_queued_duration_ns -= std::min(
          impl_->audio_queued_duration_ns, audio_duration_ns(impl_->audio.front()));
      impl_->audio.pop_front();
      ++impl_->audio_dropped_total;
    }
    impl_->audio_queued_duration_ns += audio_duration_ns(owned);
    impl_->audio.push_back(std::move(owned));
  } catch (...) {
  }
}

void SharedSession::on_data_message(const DataMessageView& message) noexcept {
  try {
    if (message.remote_uid != allowed_remote_uid_ || message.data == nullptr ||
        message.size == 0 || message.size > 1024) return;
    OwnedDataMessage owned{{message.data, message.data + message.size},
                           message.remote_uid, message.stream_id,
                           message.sent_timestamp_ms};
    std::lock_guard<std::mutex> lock(impl_->mutex);
    if (impl_->data.size() == kMaximumDataMessages) impl_->data.pop_front();
    impl_->data.push_back(std::move(owned));
  } catch (...) {
  }
}

void SharedSession::on_connection_state(ConnectionState, int) noexcept {}
void SharedSession::on_rejoined(std::uint32_t, int) noexcept {}
void SharedSession::on_connection_lost() noexcept {
  std::fprintf(stderr, "[MUXIVA][AGORA][connection.lost]\n");
}
void SharedSession::on_token_expiring() noexcept {
  std::fprintf(stderr, "[MUXIVA][AGORA][token.expiring] restart-or-renew-required=true\n");
}
void SharedSession::on_token_required() noexcept {
  std::fprintf(stderr, "[MUXIVA][AGORA][token.required]\n");
}
void SharedSession::on_network_quality(std::uint32_t, int, int) noexcept {}
void SharedSession::on_rtc_stats(const RtcStatsSnapshot&) noexcept {}
void SharedSession::on_participant_joined(std::uint32_t) noexcept {}
void SharedSession::on_participant_left(std::uint32_t, int) noexcept {}
void SharedSession::on_error(int code) noexcept {
  std::fprintf(stderr, "[MUXIVA][AGORA][session.error] code=%d\n", code);
}
void SharedSession::on_video_frame(const I420FrameView&) noexcept {}

}  // namespace muxiva::agora
