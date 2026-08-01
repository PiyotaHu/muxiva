#ifndef VOXA_AGORA_RTC_HPP
#define VOXA_AGORA_RTC_HPP

#include "voxa/voxa.h"

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

namespace voxa::agora {

struct Status final {
  int code = 0;
  std::string message;

  explicit operator bool() const noexcept { return code == 0; }
  static Status success() noexcept { return {}; }
  static Status failure(int value, const char* text) noexcept;
};

enum class ConnectionState : std::uint32_t {
  disconnected = 1,
  connecting = 2,
  connected = 3,
  reconnecting = 4,
  failed = 5,
};

struct Pcm16FrameView final {
  const std::uint8_t* data = nullptr;
  std::size_t size = 0;
  std::uint32_t sample_rate_hz = 0;
  std::uint16_t channels = 0;
  std::uint64_t samples_per_channel = 0;
  std::int64_t timestamp_ms = 0;
  std::uint32_t remote_uid = 0;
};

struct I420FrameView final {
  const std::uint8_t* y = nullptr;
  const std::uint8_t* u = nullptr;
  const std::uint8_t* v = nullptr;
  std::size_t y_stride = 0;
  std::size_t u_stride = 0;
  std::size_t v_stride = 0;
  std::uint32_t width = 0;
  std::uint32_t height = 0;
  std::int64_t timestamp_ms = 0;
  std::uint32_t remote_uid = 0;
};

class SdkObserver {
 public:
  virtual ~SdkObserver() = default;
  virtual void on_connection_state(ConnectionState state, int reason) noexcept = 0;
  virtual void on_participant_joined(std::uint32_t uid) noexcept = 0;
  virtual void on_participant_left(std::uint32_t uid, int reason) noexcept = 0;
  virtual void on_error(int code) noexcept = 0;
  virtual void on_audio_frame(const Pcm16FrameView& frame) noexcept = 0;
  virtual void on_video_frame(const I420FrameView& frame) noexcept = 0;
};

// The implementation owns Agora's IRtcEngine and must serialize all calls on
// one SDK control thread. shutdown() must synchronously stop future callbacks.
class Sdk {
 public:
  virtual ~Sdk() = default;
  virtual int initialize(const std::string& app_id, SdkObserver* observer) noexcept = 0;
  virtual int join(const std::string& token, const std::string& channel,
                   std::uint32_t uid) noexcept = 0;
  virtual int leave() noexcept = 0;
  virtual int push_audio(const Pcm16FrameView& frame) noexcept = 0;
  virtual int push_video(const I420FrameView& frame) noexcept = 0;
  virtual void shutdown() noexcept = 0;
};

struct AdapterConfig final {
  voxa_session_ingress_v1 ingress{};
  std::size_t max_packet_bytes = 4U * 1024U * 1024U;
  std::chrono::milliseconds callback_drain_timeout{2000};
};

struct AdapterStats final {
  std::uint64_t accepted = 0;
  std::uint64_t full_dropped = 0;
  std::uint64_t closed_dropped = 0;
  std::uint64_t invalid_dropped = 0;
  std::uint64_t late_dropped = 0;
  std::uint64_t outbound_audio = 0;
  std::uint64_t outbound_video = 0;
  std::uint64_t in_flight = 0;
  std::uint64_t last_sequence = 0;
};

class RtcAdapter final : private SdkObserver {
 public:
  static std::unique_ptr<RtcAdapter> create(AdapterConfig config,
                                             std::unique_ptr<Sdk> sdk,
                                             Status* status) noexcept;

  RtcAdapter(const RtcAdapter&) = delete;
  RtcAdapter& operator=(const RtcAdapter&) = delete;
  ~RtcAdapter() noexcept;

  Status connect(const std::string& app_id, const std::string& token,
                 const std::string& channel, std::uint32_t uid) noexcept;
  Status send_audio(const Pcm16FrameView& frame) noexcept;
  Status send_video(const I420FrameView& frame) noexcept;
  Status leave() noexcept;
  AdapterStats stats() const noexcept;

 private:
  struct Impl;
  explicit RtcAdapter(std::unique_ptr<Impl> impl) noexcept;

  void on_connection_state(ConnectionState state, int reason) noexcept override;
  void on_participant_joined(std::uint32_t uid) noexcept override;
  void on_participant_left(std::uint32_t uid, int reason) noexcept override;
  void on_error(int code) noexcept override;
  void on_audio_frame(const Pcm16FrameView& frame) noexcept override;
  void on_video_frame(const I420FrameView& frame) noexcept override;

  std::unique_ptr<Impl> impl_;
};

// Defined by agora_native_sdk.cc when Voxa is built with VOXA_ENABLE_AGORA.
std::unique_ptr<Sdk> make_native_sdk() noexcept;

}  // namespace voxa::agora

#endif
