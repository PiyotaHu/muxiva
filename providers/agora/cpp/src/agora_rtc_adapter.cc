#include "voxa/agora_rtc.hpp"

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstring>
#include <limits>
#include <mutex>
#include <utility>
#include <vector>

namespace voxa::agora {
namespace {

constexpr int kInvalidArgument = -1001;
constexpr int kInvalidState = -1002;
constexpr int kDrainTimeout = -1003;

std::int64_t timestamp_ns(std::int64_t milliseconds) noexcept {
  constexpr auto scale = std::int64_t{1000000};
  if (milliseconds > std::numeric_limits<std::int64_t>::max() / scale) {
    return std::numeric_limits<std::int64_t>::max();
  }
  if (milliseconds < std::numeric_limits<std::int64_t>::min() / scale) {
    return std::numeric_limits<std::int64_t>::min();
  }
  return milliseconds * scale;
}

bool valid_text(const std::string& value, std::size_t maximum,
                bool allow_empty = false) noexcept {
  return value.size() <= maximum && (allow_empty || !value.empty());
}

bool checked_product(std::size_t left, std::size_t right,
                     std::size_t* out) noexcept {
  if (left != 0 && right > std::numeric_limits<std::size_t>::max() / left) {
    return false;
  }
  *out = left * right;
  return true;
}

}  // namespace

Status Status::failure(int value, const char* text) noexcept {
  try {
    return {value, text == nullptr ? std::string{} : std::string{text}};
  } catch (...) {
    return {value, {}};
  }
}

struct RtcAdapter::Impl final {
  explicit Impl(AdapterConfig value, std::unique_ptr<Sdk> provider,
                voxa_session_ingress_v1 retained) noexcept
      : config(value), sdk(std::move(provider)), ingress(retained) {}

  ~Impl() { (void)voxa_session_ingress_release_v1(ingress); }

  struct Flight final {
    explicit Flight(Impl& value) noexcept : impl(value) {
      impl.in_flight.fetch_add(1, std::memory_order_acq_rel);
    }
    ~Flight() {
      impl.in_flight.fetch_sub(1, std::memory_order_acq_rel);
      impl.drain_cv.notify_all();
    }
    Impl& impl;
  };

  std::uint64_t next_sequence() noexcept {
    return sequence.fetch_add(1, std::memory_order_relaxed) + 1;
  }

  void account(voxa_status_v1 status, std::uint64_t current) noexcept {
    if (status == VOXA_STATUS_OK) {
      accepted.fetch_add(1, std::memory_order_relaxed);
    } else if (status == VOXA_STATUS_QUEUE_FULL || status == VOXA_STATUS_BUSY) {
      full.fetch_add(1, std::memory_order_relaxed);
    } else if (status == VOXA_STATUS_CLOSED) {
      closed.fetch_add(1, std::memory_order_relaxed);
    } else {
      invalid.fetch_add(1, std::memory_order_relaxed);
    }
    last_sequence.store(current, std::memory_order_release);
  }

  void submit_control(bool event, const char* name, const std::string& value) noexcept {
    Flight flight(*this);
    if (!accepting.load(std::memory_order_acquire)) {
      late.fetch_add(1, std::memory_order_relaxed);
      return;
    }
    const auto current = next_sequence();
    voxa_frame_view_v1 frame{};
    frame.header.abi_version = VOXA_ABI_VERSION_V1;
    frame.header.struct_size = sizeof(frame.header);
    frame.header.frame_type = event ? VOXA_FRAME_EVENT : VOXA_FRAME_SIGNAL;
    frame.header.clock_kind = VOXA_CLOCK_MONOTONIC;
    frame.header.sequence_id = current;
    static constexpr char frame_id[] = "agora.control";
    static constexpr char clock_id[] = "agora.control.clock";
    static constexpr char stream_id[] = "agora.control.stream";
    static constexpr char trace_id[] = "agora.control.trace";
    frame.header.frame_id = {frame_id, sizeof(frame_id) - 1};
    frame.header.clock_domain_id = {clock_id, sizeof(clock_id) - 1};
    frame.header.stream_id = {stream_id, sizeof(stream_id) - 1};
    frame.header.trace_id = {trace_id, sizeof(trace_id) - 1};
    const voxa_bytes_v1 bytes{reinterpret_cast<const std::uint8_t*>(value.data()),
                              value.size()};
    if (event) {
      frame.payload.event = {{name, std::strlen(name)}, bytes, {0, 0}};
    } else {
      static constexpr char source[] = "agora.adapter";
      frame.payload.signal = {{name, std::strlen(name)},
                              {source, sizeof(source) - 1}, bytes, {0, 0}};
    }
    voxa_error_v1 error{};
    error.abi_version = VOXA_ABI_VERSION_V1;
    error.struct_size = sizeof(error);
    account(voxa_session_ingress_try_submit_v1(ingress, &frame, &error), current);
  }

  AdapterConfig config;
  std::unique_ptr<Sdk> sdk;
  voxa_session_ingress_v1 ingress{};
  mutable std::mutex lifecycle;
  std::mutex drain_mutex;
  std::condition_variable drain_cv;
  std::atomic<bool> accepting{false};
  std::atomic<bool> closed_once{false};
  std::atomic<std::uint64_t> sequence{0};
  std::atomic<std::uint64_t> accepted{0};
  std::atomic<std::uint64_t> full{0};
  std::atomic<std::uint64_t> closed{0};
  std::atomic<std::uint64_t> invalid{0};
  std::atomic<std::uint64_t> late{0};
  std::atomic<std::uint64_t> outbound_audio{0};
  std::atomic<std::uint64_t> outbound_video{0};
  std::atomic<std::uint64_t> in_flight{0};
  std::atomic<std::uint64_t> last_sequence{0};
  std::atomic<std::uint64_t> connection_epoch{0};
  std::atomic<std::uint64_t> reconnects{0};
  std::atomic<std::uint64_t> connection_losses{0};
  std::atomic<std::uint64_t> token_expiring{0};
  std::atomic<std::uint64_t> token_required{0};
  std::atomic<std::uint64_t> token_renewals{0};
  std::atomic<std::uint64_t> token_renewal_failures{0};
  std::atomic<std::uint64_t> network_quality_samples{0};
  std::atomic<std::uint64_t> rtc_stats_samples{0};
  std::atomic<std::uint32_t> rtc_duration_seconds{0};
  std::atomic<std::uint64_t> rtc_tx_bytes{0};
  std::atomic<std::uint64_t> rtc_rx_bytes{0};
  std::atomic<std::uint32_t> rtc_user_count{0};
  std::atomic<std::uint32_t> rtc_lastmile_delay_ms{0};
  std::atomic<int> worst_tx_quality{0};
  std::atomic<int> worst_rx_quality{0};
  std::atomic<ConnectionState> connection_state{ConnectionState::disconnected};
  std::atomic<bool> reconnecting{false};
};

std::unique_ptr<RtcAdapter> RtcAdapter::create(AdapterConfig config,
                                                std::unique_ptr<Sdk> sdk,
                                                Status* status) noexcept {
  auto fail = [&](int code, const char* message) {
    if (status) *status = Status::failure(code, message);
    return std::unique_ptr<RtcAdapter>{};
  };
  if (!sdk || config.max_packet_bytes == 0 ||
      config.max_packet_bytes > 64U * 1024U * 1024U ||
      config.callback_drain_timeout.count() < 0) {
    return fail(kInvalidArgument, "invalid Agora adapter configuration");
  }
  voxa_session_ingress_v1 retained{};
  voxa_error_v1 error{};
  error.abi_version = VOXA_ABI_VERSION_V1;
  error.struct_size = sizeof(error);
  if (voxa_session_ingress_clone_v1(config.ingress, &retained, &error) != VOXA_STATUS_OK) {
    return fail(kInvalidArgument, "failed to retain Voxa ingress");
  }
  try {
    auto impl = std::make_unique<Impl>(config, std::move(sdk), retained);
    auto adapter = std::unique_ptr<RtcAdapter>(new RtcAdapter(std::move(impl)));
    if (status) *status = Status::success();
    return adapter;
  } catch (...) {
    (void)voxa_session_ingress_release_v1(retained);
    return fail(kInvalidArgument, "failed to allocate Agora adapter");
  }
}

RtcAdapter::RtcAdapter(std::unique_ptr<Impl> impl) noexcept : impl_(std::move(impl)) {}

RtcAdapter::~RtcAdapter() noexcept { (void)leave(); }

Status RtcAdapter::connect(const std::string& app_id, const std::string& token,
                           const std::string& channel, std::uint32_t uid) noexcept {
  if (!valid_text(app_id, 64) || !valid_text(token, 4096, true) ||
      !valid_text(channel, 64)) {
    return Status::failure(kInvalidArgument, "invalid Agora join fields");
  }
  std::lock_guard<std::mutex> lock(impl_->lifecycle);
  if (impl_->closed_once.load(std::memory_order_acquire) ||
      impl_->accepting.load(std::memory_order_acquire)) {
    return Status::failure(kInvalidState, "Agora adapter cannot connect in its current state");
  }
  if (const int result = impl_->sdk->initialize(app_id, this); result != 0) {
    impl_->sdk->shutdown();
    impl_->closed_once.store(true, std::memory_order_release);
    (void)voxa_session_ingress_close_v1(impl_->ingress);
    return Status::failure(result, "Agora SDK initialization failed");
  }
  impl_->accepting.store(true, std::memory_order_release);
  if (const int result = impl_->sdk->join(token, channel, uid); result != 0) {
    impl_->accepting.store(false, std::memory_order_release);
    impl_->sdk->shutdown();
    impl_->closed_once.store(true, std::memory_order_release);
    (void)voxa_session_ingress_close_v1(impl_->ingress);
    return Status::failure(result, "Agora SDK join failed");
  }
  return Status::success();
}

Status RtcAdapter::send_audio(const Pcm16FrameView& frame) noexcept {
  std::size_t samples = 0;
  std::size_t expected = 0;
  if (!checked_product(static_cast<std::size_t>(frame.samples_per_channel),
                       frame.channels, &samples) ||
      !checked_product(samples, std::size_t{2}, &expected) || expected != frame.size ||
      expected > impl_->config.max_packet_bytes || frame.data == nullptr ||
      frame.sample_rate_hz == 0 || frame.channels == 0) {
    return Status::failure(kInvalidArgument, "invalid PCM16 audio frame");
  }
  if (!impl_->accepting.load(std::memory_order_acquire)) {
    return Status::failure(kInvalidState, "Agora adapter is not connected");
  }
  const int result = impl_->sdk->push_audio(frame);
  if (result != 0) return Status::failure(result, "Agora audio publish failed");
  impl_->outbound_audio.fetch_add(1, std::memory_order_relaxed);
  return Status::success();
}

Status RtcAdapter::send_video(const I420FrameView& frame) noexcept {
  if (frame.width == 0 || frame.height == 0 || frame.width % 2 != 0 ||
      frame.height % 2 != 0 || frame.y == nullptr || frame.u == nullptr ||
      frame.v == nullptr || frame.y_stride < frame.width ||
      frame.u_stride < frame.width / 2 || frame.v_stride < frame.width / 2) {
    return Status::failure(kInvalidArgument, "invalid I420 video frame");
  }
  std::size_t pixels = 0;
  if (!checked_product(frame.width, frame.height, &pixels) ||
      pixels > impl_->config.max_packet_bytes ||
      pixels / 2 > impl_->config.max_packet_bytes - pixels) {
    return Status::failure(kInvalidArgument, "I420 video frame exceeds packet budget");
  }
  if (!impl_->accepting.load(std::memory_order_acquire)) {
    return Status::failure(kInvalidState, "Agora adapter is not connected");
  }
  const int result = impl_->sdk->push_video(frame);
  if (result != 0) return Status::failure(result, "Agora video publish failed");
  impl_->outbound_video.fetch_add(1, std::memory_order_relaxed);
  return Status::success();
}

Status RtcAdapter::renew_token(const std::string& token) noexcept {
  if (!valid_text(token, 4096)) {
    return Status::failure(kInvalidArgument, "invalid Agora token");
  }
  std::lock_guard<std::mutex> lock(impl_->lifecycle);
  if (impl_->closed_once.load(std::memory_order_acquire) ||
      !impl_->accepting.load(std::memory_order_acquire)) {
    return Status::failure(kInvalidState, "Agora adapter is not connected");
  }
  const int result = impl_->sdk->renew_token(token);
  if (result != 0) {
    impl_->token_renewal_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(result, "Agora token renewal failed");
  }
  impl_->token_renewals.fetch_add(1, std::memory_order_relaxed);
  return Status::success();
}

Status RtcAdapter::leave() noexcept {
  int leave_result = 0;
  bool owns_shutdown = false;
  {
    std::lock_guard<std::mutex> lock(impl_->lifecycle);
    if (!impl_->closed_once.exchange(true, std::memory_order_acq_rel)) {
      owns_shutdown = true;
      impl_->accepting.store(false, std::memory_order_release);
      (void)voxa_session_ingress_close_v1(impl_->ingress);
      leave_result = impl_->sdk->leave();
      impl_->sdk->shutdown();
    }
  }
  std::unique_lock<std::mutex> lock(impl_->drain_mutex);
  const bool drained = impl_->drain_cv.wait_for(
      lock, impl_->config.callback_drain_timeout,
      [&] { return impl_->in_flight.load(std::memory_order_acquire) == 0; });
  if (!drained) return Status::failure(kDrainTimeout, "Agora callback drain timed out");
  if (owns_shutdown && leave_result != 0) {
    return Status::failure(leave_result, "Agora SDK leave failed");
  }
  return Status::success();
}

AdapterStats RtcAdapter::stats() const noexcept {
  return {impl_->accepted.load(std::memory_order_relaxed),
          impl_->full.load(std::memory_order_relaxed),
          impl_->closed.load(std::memory_order_relaxed),
          impl_->invalid.load(std::memory_order_relaxed),
          impl_->late.load(std::memory_order_relaxed),
          impl_->outbound_audio.load(std::memory_order_relaxed),
          impl_->outbound_video.load(std::memory_order_relaxed),
          impl_->in_flight.load(std::memory_order_relaxed),
          impl_->last_sequence.load(std::memory_order_relaxed),
          impl_->connection_epoch.load(std::memory_order_relaxed),
          impl_->reconnects.load(std::memory_order_relaxed),
          impl_->connection_losses.load(std::memory_order_relaxed),
          impl_->token_expiring.load(std::memory_order_relaxed),
          impl_->token_required.load(std::memory_order_relaxed),
          impl_->token_renewals.load(std::memory_order_relaxed),
          impl_->token_renewal_failures.load(std::memory_order_relaxed),
          impl_->network_quality_samples.load(std::memory_order_relaxed),
          impl_->rtc_stats_samples.load(std::memory_order_relaxed),
          {impl_->rtc_duration_seconds.load(std::memory_order_relaxed),
           impl_->rtc_tx_bytes.load(std::memory_order_relaxed),
           impl_->rtc_rx_bytes.load(std::memory_order_relaxed),
           impl_->rtc_user_count.load(std::memory_order_relaxed),
           impl_->rtc_lastmile_delay_ms.load(std::memory_order_relaxed)},
          impl_->worst_tx_quality.load(std::memory_order_relaxed),
          impl_->worst_rx_quality.load(std::memory_order_relaxed),
          impl_->connection_state.load(std::memory_order_relaxed)};
}

void RtcAdapter::on_connection_state(ConnectionState state, int reason) noexcept {
  try {
    const auto previous =
        impl_->connection_state.exchange(state, std::memory_order_acq_rel);
    if (state == ConnectionState::reconnecting) {
      impl_->reconnecting.store(true, std::memory_order_release);
    }
    if (state == ConnectionState::connected) {
      const bool was_reconnecting =
          impl_->reconnecting.exchange(false, std::memory_order_acq_rel);
      if (was_reconnecting) {
        impl_->reconnects.fetch_add(1, std::memory_order_relaxed);
      }
      if (previous != ConnectionState::connected) {
        impl_->connection_epoch.fetch_add(1, std::memory_order_relaxed);
      }
    }
    impl_->submit_control(false, "agora.connection_state",
                          "{\"schema_version\":1,\"state\":" +
                              std::to_string(static_cast<std::uint32_t>(state)) +
                              ",\"previous_state\":" +
                              std::to_string(static_cast<std::uint32_t>(previous)) +
                              ",\"reason\":" + std::to_string(reason) +
                              ",\"epoch\":" +
                              std::to_string(impl_->connection_epoch.load(
                                  std::memory_order_relaxed)) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_rejoined(std::uint32_t uid, int elapsed_ms) noexcept {
  try {
    impl_->submit_control(false, "agora.rejoined",
                          "{\"schema_version\":1,\"uid\":" +
                              std::to_string(uid) + ",\"elapsed_ms\":" +
                              std::to_string(elapsed_ms) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_connection_lost() noexcept {
  impl_->connection_losses.fetch_add(1, std::memory_order_relaxed);
  impl_->reconnecting.store(true, std::memory_order_release);
  impl_->submit_control(true, "agora.connection_lost",
                        "{\"schema_version\":1}");
}

void RtcAdapter::on_token_expiring() noexcept {
  impl_->token_expiring.fetch_add(1, std::memory_order_relaxed);
  impl_->submit_control(true, "agora.token",
                        "{\"schema_version\":1,\"kind\":\"will_expire\"}");
}

void RtcAdapter::on_token_required() noexcept {
  impl_->token_required.fetch_add(1, std::memory_order_relaxed);
  impl_->submit_control(true, "agora.token",
                        "{\"schema_version\":1,\"kind\":\"required\"}");
}

void RtcAdapter::on_network_quality(std::uint32_t uid, int tx_quality,
                                    int rx_quality) noexcept {
  impl_->network_quality_samples.fetch_add(1, std::memory_order_relaxed);
  int current = impl_->worst_tx_quality.load(std::memory_order_relaxed);
  while (tx_quality > current &&
         !impl_->worst_tx_quality.compare_exchange_weak(
             current, tx_quality, std::memory_order_relaxed)) {}
  current = impl_->worst_rx_quality.load(std::memory_order_relaxed);
  while (rx_quality > current &&
         !impl_->worst_rx_quality.compare_exchange_weak(
             current, rx_quality, std::memory_order_relaxed)) {}
  try {
    impl_->submit_control(true, "agora.network_quality",
                          "{\"schema_version\":1,\"uid\":" +
                              std::to_string(uid) + ",\"tx_quality\":" +
                              std::to_string(tx_quality) + ",\"rx_quality\":" +
                              std::to_string(rx_quality) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_rtc_stats(const RtcStatsSnapshot& stats) noexcept {
  impl_->rtc_stats_samples.fetch_add(1, std::memory_order_relaxed);
  impl_->rtc_duration_seconds.store(stats.duration_seconds,
                                    std::memory_order_relaxed);
  impl_->rtc_tx_bytes.store(stats.tx_bytes, std::memory_order_relaxed);
  impl_->rtc_rx_bytes.store(stats.rx_bytes, std::memory_order_relaxed);
  impl_->rtc_user_count.store(stats.user_count, std::memory_order_relaxed);
  impl_->rtc_lastmile_delay_ms.store(stats.lastmile_delay_ms,
                                     std::memory_order_relaxed);
  try {
    impl_->submit_control(true, "agora.rtc_stats",
                          "{\"schema_version\":1,\"duration_seconds\":" +
                              std::to_string(stats.duration_seconds) +
                              ",\"tx_bytes\":" + std::to_string(stats.tx_bytes) +
                              ",\"rx_bytes\":" + std::to_string(stats.rx_bytes) +
                              ",\"user_count\":" +
                              std::to_string(stats.user_count) +
                              ",\"lastmile_delay_ms\":" +
                              std::to_string(stats.lastmile_delay_ms) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_participant_joined(std::uint32_t uid) noexcept {
  try {
    impl_->submit_control(false, "agora.participant",
                          "{\"schema_version\":1,\"kind\":\"joined\",\"uid\":" +
                              std::to_string(uid) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_participant_left(std::uint32_t uid, int reason) noexcept {
  try {
    impl_->submit_control(false, "agora.participant",
                          "{\"schema_version\":1,\"kind\":\"left\",\"uid\":" +
                              std::to_string(uid) + ",\"reason\":" +
                              std::to_string(reason) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_error(int code) noexcept {
  try {
    impl_->submit_control(true, "agora.error",
                          "{\"schema_version\":1,\"code\":" +
                              std::to_string(code) + "}");
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

void RtcAdapter::on_audio_frame(const Pcm16FrameView& value) noexcept {
  Impl::Flight flight(*impl_);
  if (!impl_->accepting.load(std::memory_order_acquire)) {
    impl_->late.fetch_add(1, std::memory_order_relaxed);
    return;
  }
  std::size_t samples = 0;
  std::size_t expected = 0;
  if (!checked_product(static_cast<std::size_t>(value.samples_per_channel),
                       value.channels, &samples) ||
      !checked_product(samples, std::size_t{2}, &expected) || expected != value.size ||
      expected > impl_->config.max_packet_bytes || value.data == nullptr ||
      value.sample_rate_hz == 0 || value.channels == 0) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
    return;
  }
  const auto current = impl_->next_sequence();
  voxa_frame_view_v1 frame{};
  frame.header.abi_version = VOXA_ABI_VERSION_V1;
  frame.header.struct_size = sizeof(frame.header);
  frame.header.frame_type = VOXA_FRAME_AUDIO;
  frame.header.clock_kind = VOXA_CLOCK_MEDIA_RELATIVE;
  frame.header.timestamp_ns = timestamp_ns(value.timestamp_ms);
  frame.header.sequence_id = current;
  static constexpr char frame_id[] = "agora.audio";
  static constexpr char clock_id[] = "agora.media.clock";
  static constexpr char stream_id[] = "agora.remote.audio";
  static constexpr char trace_id[] = "agora.media.trace";
  frame.header.frame_id = {frame_id, sizeof(frame_id) - 1};
  frame.header.clock_domain_id = {clock_id, sizeof(clock_id) - 1};
  frame.header.stream_id = {stream_id, sizeof(stream_id) - 1};
  frame.header.trace_id = {trace_id, sizeof(trace_id) - 1};
  frame.payload.audio = {value.sample_rate_hz, value.channels, VOXA_PCM_I16LE,
                         VOXA_AUDIO_INTERLEAVED, 0, value.samples_per_channel,
                         {value.data, value.size}, {0, 0}};
  voxa_error_v1 error{};
  error.abi_version = VOXA_ABI_VERSION_V1;
  error.struct_size = sizeof(error);
  impl_->account(voxa_session_ingress_try_submit_v1(impl_->ingress, &frame, &error),
                 current);
}

void RtcAdapter::on_video_frame(const I420FrameView& value) noexcept {
  Impl::Flight flight(*impl_);
  if (!impl_->accepting.load(std::memory_order_acquire)) {
    impl_->late.fetch_add(1, std::memory_order_relaxed);
    return;
  }
  if (value.width == 0 || value.height == 0 || value.width % 2 != 0 ||
      value.height % 2 != 0 || value.y == nullptr || value.u == nullptr ||
      value.v == nullptr || value.y_stride < value.width ||
      value.u_stride < value.width / 2 || value.v_stride < value.width / 2) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
    return;
  }
  std::size_t pixels = 0;
  if (!checked_product(value.width, value.height, &pixels) ||
      pixels > impl_->config.max_packet_bytes ||
      pixels / 2 > impl_->config.max_packet_bytes - pixels) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
    return;
  }
  try {
    std::vector<std::uint8_t> packed(pixels + pixels / 2);
    auto* output = packed.data();
    for (std::uint32_t row = 0; row < value.height; ++row) {
      std::memcpy(output + static_cast<std::size_t>(row) * value.width,
                  value.y + static_cast<std::size_t>(row) * value.y_stride,
                  value.width);
    }
    output += pixels;
    const auto chroma_width = value.width / 2;
    const auto chroma_height = value.height / 2;
    for (std::uint32_t row = 0; row < chroma_height; ++row) {
      std::memcpy(output + static_cast<std::size_t>(row) * chroma_width,
                  value.u + static_cast<std::size_t>(row) * value.u_stride,
                  chroma_width);
    }
    output += pixels / 4;
    for (std::uint32_t row = 0; row < chroma_height; ++row) {
      std::memcpy(output + static_cast<std::size_t>(row) * chroma_width,
                  value.v + static_cast<std::size_t>(row) * value.v_stride,
                  chroma_width);
    }
    const auto current = impl_->next_sequence();
    voxa_frame_view_v1 frame{};
    frame.header.abi_version = VOXA_ABI_VERSION_V1;
    frame.header.struct_size = sizeof(frame.header);
    frame.header.frame_type = VOXA_FRAME_VIDEO;
    frame.header.clock_kind = VOXA_CLOCK_MEDIA_RELATIVE;
    frame.header.timestamp_ns = timestamp_ns(value.timestamp_ms);
    frame.header.sequence_id = current;
    static constexpr char frame_id[] = "agora.video";
    static constexpr char clock_id[] = "agora.media.clock";
    static constexpr char stream_id[] = "agora.remote.video";
    static constexpr char trace_id[] = "agora.media.trace";
    frame.header.frame_id = {frame_id, sizeof(frame_id) - 1};
    frame.header.clock_domain_id = {clock_id, sizeof(clock_id) - 1};
    frame.header.stream_id = {stream_id, sizeof(stream_id) - 1};
    frame.header.trace_id = {trace_id, sizeof(trace_id) - 1};
    frame.payload.video = {value.width, value.height, VOXA_PIXEL_I420, 3,
                           {packed.data(), packed.size()}, {0, 0, 0, 0}};
    voxa_error_v1 error{};
    error.abi_version = VOXA_ABI_VERSION_V1;
    error.struct_size = sizeof(error);
    impl_->account(
        voxa_session_ingress_try_submit_v1(impl_->ingress, &frame, &error), current);
  } catch (...) {
    impl_->invalid.fetch_add(1, std::memory_order_relaxed);
  }
}

}  // namespace voxa::agora
