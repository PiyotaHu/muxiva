#include <muxiva/agora_rtc.hpp>
#include <muxiva/muxiva.hpp>

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0')
    throw std::runtime_error(std::string("missing ") + name);
  return value;
}

std::string base64_encode(const std::uint8_t* data, std::size_t size) {
  static constexpr char alphabet[] =
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  std::string encoded;
  encoded.reserve(((size + 2U) / 3U) * 4U);
  for (std::size_t offset = 0; offset < size; offset += 3U) {
    const std::uint32_t first = data[offset];
    const std::uint32_t second = offset + 1U < size ? data[offset + 1U] : 0U;
    const std::uint32_t third = offset + 2U < size ? data[offset + 2U] : 0U;
    const std::uint32_t value = (first << 16U) | (second << 8U) | third;
    encoded.push_back(alphabet[(value >> 18U) & 0x3fU]);
    encoded.push_back(alphabet[(value >> 12U) & 0x3fU]);
    encoded.push_back(offset + 1U < size ? alphabet[(value >> 6U) & 0x3fU] : '=');
    encoded.push_back(offset + 2U < size ? alphabet[value & 0x3fU] : '=');
  }
  return encoded;
}

std::vector<std::string> transport_packets(const std::uint8_t* data,
                                           std::size_t size,
                                           std::uint64_t message_number) {
  constexpr std::size_t kPacketLimit = 1024U;
  constexpr std::size_t kFragmentBytes = 512U;
  constexpr std::size_t kMaximumFragments = 64U;
  if (size == 0U || data == nullptr)
    throw std::invalid_argument("Agora data Sink requires a non-empty message");
  if (size <= kPacketLimit)
    return {std::string(reinterpret_cast<const char*>(data), size)};
  const std::size_t fragment_count =
      (size + kFragmentBytes - 1U) / kFragmentBytes;
  if (fragment_count > kMaximumFragments)
    throw std::invalid_argument(
        "Agora data Sink message exceeds the 32 KiB transport limit");

  const std::string message_id = "agora-" + std::to_string(message_number);
  std::vector<std::string> packets;
  packets.reserve(fragment_count);
  for (std::size_t index = 0; index < fragment_count; ++index) {
    const std::size_t offset = index * kFragmentBytes;
    const std::size_t length = std::min(kFragmentBytes, size - offset);
    std::string packet =
        "{\"version\":\"muxiva.transport-fragment/v1\",\"message_id\":\"" +
        message_id + "\",\"fragment_index\":" + std::to_string(index) +
        ",\"fragment_count\":" + std::to_string(fragment_count) +
        ",\"encoding\":\"base64\",\"data\":\"" +
        base64_encode(data + offset, length) + "\"}";
    if (packet.size() > kPacketLimit)
      throw std::runtime_error("Agora transport fragment exceeds its packet limit");
    packets.push_back(std::move(packet));
  }
  return packets;
}

class AgoraDataSinkNode final : public muxiva::MultimodalGraphNode {
 public:
  void on_prepare() override {
    const auto uid = static_cast<std::uint32_t>(
        std::stoul(required_env("MUXIVA_AGORA_BOT_UID")));
    const auto participant = static_cast<std::uint32_t>(
        std::stoul(required_env("MUXIVA_AGORA_WEB_UID")));
    session_ = muxiva::agora::SharedSession::acquire(
        required_env("MUXIVA_AGORA_APP_ID"),
        required_env("MUXIVA_AGORA_BOT_TOKEN"),
        required_env("MUXIVA_AGORA_CHANNEL"), uid, participant);
    next_send_ = std::chrono::steady_clock::now();
  }

  void on_process(const muxiva_frame_view_v1* input,
                  muxiva::GraphNodeContext&) override {
    if (input == nullptr || input->header.frame_type != MUXIVA_FRAME_BYTE)
      throw std::invalid_argument("Agora data Sink requires a Byte Frame");
    const auto& payload = input->payload.bytes.bytes;
    const auto packets = transport_packets(
        payload.data, payload.len, ++logical_messages_);
    for (const auto& packet : packets)
      send_packet(reinterpret_cast<const std::uint8_t*>(packet.data()), packet.size());
  }

  void send_packet(const std::uint8_t* data, std::size_t size) {
    const auto now = std::chrono::steady_clock::now();
    if (next_send_ > now) std::this_thread::sleep_until(next_send_);
    const int result = session_ ? session_->send_data(data, size) : -7;
    const auto count = ++published_;
    if (count == 1 || count % 25 == 0 || result != 0) {
      std::fprintf(stderr,
                   "[MUXIVA][AGORA][data.published] messages=%llu bytes=%zu result=%d\n",
                   static_cast<unsigned long long>(count), size, result);
    }
    if (result != 0)
      throw std::runtime_error("Agora data Sink failed to publish message: code=" +
                               std::to_string(result));

    // Pace by bytes below Agora's 6 KiB/s and 30-call/s ceilings. Small
    // transcript deltas stay responsive while 1 KiB fragments remain safe.
    constexpr std::size_t kBudgetBytesPerSecond = 5U * 1024U;
    constexpr std::size_t kMaximumCallsPerSecond = 25U;
    const auto byte_delay_ms = static_cast<std::int64_t>(
        (size * 1000U + kBudgetBytesPerSecond - 1U) /
        kBudgetBytesPerSecond);
    const auto call_delay_ms = static_cast<std::int64_t>(
        (1000U + kMaximumCallsPerSecond - 1U) / kMaximumCallsPerSecond);
    next_send_ = std::chrono::steady_clock::now() + std::chrono::milliseconds(
        std::max(byte_delay_ms, call_delay_ms));
  }

  void on_finish() override { session_.reset(); }

  void on_abort(const muxiva_abort_reason_v1&) noexcept override {
    try { on_finish(); } catch (...) {}
  }

 private:
  std::shared_ptr<muxiva::agora::SharedSession> session_;
  std::chrono::steady_clock::time_point next_send_{};
  std::uint64_t logical_messages_ = 0;
  std::uint64_t published_ = 0;
};
}  // namespace

extern "C" muxiva_multimodal_node_factory_v1 muxiva_node_pack_factory() {
  static const auto factory =
      muxiva::MultimodalGraphNodeFactory::make<AgoraDataSinkNode>(
          "agora.data_sink", MUXIVA_NODE_SINK,
          R"json([{"name":"message_in","direction":"input","frameType":"byte"}])json",
          "{}", "1.0.0");
  return factory.view();
}
