#include "muxiva/agora_rtc.hpp"

#include <atomic>
#include <cassert>
#include <cstdint>
#include <memory>
#include <thread>
#include <vector>

namespace {

struct Core final {
  muxiva_runtime_v1 runtime{};
  muxiva_session_v1 session{};
  muxiva_session_ingress_v1 ingress{};
  muxiva_error_v1 error{};

  Core() {
    error.abi_version = MUXIVA_ABI_VERSION_V1;
    error.struct_size = sizeof(error);
    assert(muxiva_runtime_create_v1(&runtime, &error) == MUXIVA_STATUS_OK);
    assert(muxiva_session_create_v1(runtime, &session, &error) == MUXIVA_STATUS_OK);
    const muxiva_ingress_config_v1 config{MUXIVA_ABI_VERSION_V1, sizeof(config), 16,
                                        64U * 1024U};
    assert(muxiva_session_ingress_create_v1(session, &config, &ingress, &error) ==
           MUXIVA_STATUS_OK);
  }

  ~Core() {
    (void)muxiva_session_ingress_release_v1(ingress);
    (void)muxiva_session_release_v1(session);
    (void)muxiva_runtime_release_v1(runtime);
  }
};

struct FakeState final {
  muxiva::agora::SdkObserver* observer = nullptr;
  std::atomic<unsigned> audio_pushed{0};
  std::atomic<unsigned> video_pushed{0};
  std::atomic<unsigned> tokens_renewed{0};
  std::atomic<unsigned> shutdowns{0};
};

class FakeSdk final : public muxiva::agora::Sdk {
 public:
  explicit FakeSdk(std::shared_ptr<FakeState> state) : state_(std::move(state)) {}

  int initialize(const std::string& app_id,
                 muxiva::agora::SdkObserver* observer) noexcept override {
    if (app_id != "app" || observer == nullptr) return -1;
    state_->observer = observer;
    return 0;
  }

  int join(const std::string& token, const std::string& channel,
           std::uint32_t uid) noexcept override {
    if (token != "token" || channel != "room" || uid != 7) return -2;
    state_->observer->on_connection_state(muxiva::agora::ConnectionState::connecting, 0);
    state_->observer->on_connection_state(muxiva::agora::ConnectionState::connected, 0);
    state_->observer->on_participant_joined(42);
    return 0;
  }

  int leave() noexcept override { return 0; }
  int renew_token(const std::string& token) noexcept override {
    if (token != "fresh-token") return -9;
    ++state_->tokens_renewed;
    return 0;
  }
  int push_audio(const muxiva::agora::Pcm16FrameView&) noexcept override {
    ++state_->audio_pushed;
    return 0;
  }
  int push_video(const muxiva::agora::I420FrameView&) noexcept override {
    ++state_->video_pushed;
    return 0;
  }
  void shutdown() noexcept override { ++state_->shutdowns; }

 private:
  std::shared_ptr<FakeState> state_;
};

void drains_ingress(Core& core) {
  for (;;) {
    muxiva_frame_v1 frame{};
    const auto status =
        muxiva_session_ingress_try_pop_v1(core.ingress, &frame, &core.error);
    if (status == MUXIVA_STATUS_BUSY) return;
    assert(status == MUXIVA_STATUS_OK);
    assert(muxiva_frame_release_v1(frame) == MUXIVA_STATUS_OK);
  }
}

}  // namespace

int main() {
  Core core;
  auto state = std::make_shared<FakeState>();
  muxiva::agora::AdapterConfig config;
  config.ingress = core.ingress;
  config.max_packet_bytes = 4096;
  muxiva::agora::Status create_status;
  auto adapter = muxiva::agora::RtcAdapter::create(
      config, std::make_unique<FakeSdk>(state), &create_status);
  assert(adapter && create_status);
  assert(adapter->connect("app", "token", "room", 7));

  std::vector<std::uint8_t> audio(320, 7);
  const muxiva::agora::Pcm16FrameView audio_frame{
      audio.data(), audio.size(), 16000, 1, 160, 10, 42};
  std::vector<std::uint8_t> y(16, 16), u(4, 128), v(4, 128);
  const muxiva::agora::I420FrameView video_frame{
      y.data(), u.data(), v.data(), 4, 2, 2, 4, 4, 11, 42};

  std::thread callback([&] {
    state->observer->on_audio_frame(audio_frame);
    state->observer->on_video_frame(video_frame);
  });
  callback.join();
  std::fill(audio.begin(), audio.end(), 0);
  std::fill(y.begin(), y.end(), 0);

  assert(adapter->send_audio(audio_frame));
  assert(adapter->send_video(video_frame));
  assert(adapter->renew_token("fresh-token"));
  assert(!adapter->renew_token("rejected-token"));
  state->observer->on_token_expiring();
  state->observer->on_token_required();
  state->observer->on_network_quality(42, 3, 5);
  state->observer->on_rtc_stats({60, 1000, 2000, 2, 45});
  state->observer->on_connection_state(
      muxiva::agora::ConnectionState::reconnecting, 2);
  state->observer->on_connection_lost();
  state->observer->on_connection_state(muxiva::agora::ConnectionState::connected, 0);
  state->observer->on_rejoined(7, 123);
  const auto before_leave = adapter->stats();
  assert(before_leave.accepted == 13);
  assert(before_leave.invalid_dropped == 0);
  assert(before_leave.outbound_audio == 1);
  assert(before_leave.outbound_video == 1);
  assert(before_leave.connection_epoch == 2);
  assert(before_leave.reconnects == 1);
  assert(before_leave.connection_losses == 1);
  assert(before_leave.token_expiring == 1);
  assert(before_leave.token_required == 1);
  assert(before_leave.token_renewals == 1);
  assert(before_leave.token_renewal_failures == 1);
  assert(before_leave.network_quality_samples == 1);
  assert(before_leave.rtc_stats_samples == 1);
  assert(before_leave.latest_rtc.duration_seconds == 60);
  assert(before_leave.latest_rtc.tx_bytes == 1000);
  assert(before_leave.latest_rtc.rx_bytes == 2000);
  assert(before_leave.latest_rtc.user_count == 2);
  assert(before_leave.latest_rtc.lastmile_delay_ms == 45);
  assert(before_leave.worst_tx_quality == 3);
  assert(before_leave.worst_rx_quality == 5);
  assert(before_leave.connection_state == muxiva::agora::ConnectionState::connected);
  assert(state->audio_pushed == 1);
  assert(state->video_pushed == 1);
  assert(state->tokens_renewed == 1);

  drains_ingress(core);
  std::atomic<bool> first_left{false};
  std::atomic<bool> second_left{false};
  std::thread first_leave([&] { first_left = static_cast<bool>(adapter->leave()); });
  std::thread second_leave([&] { second_left = static_cast<bool>(adapter->leave()); });
  first_leave.join();
  second_leave.join();
  assert(first_left && second_left);
  assert(adapter->leave());
  assert(state->shutdowns == 1);

  // A provider that violates shutdown's no-late-callback contract is still
  // contained while the adapter owner is alive.
  state->observer->on_audio_frame(audio_frame);
  assert(adapter->stats().late_dropped == 1);
  assert(!adapter->send_audio(audio_frame));
  assert(!adapter->renew_token("fresh-token"));
}
