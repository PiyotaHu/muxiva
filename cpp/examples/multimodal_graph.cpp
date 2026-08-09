#include <muxiva/muxiva.hpp>

#include <atomic>
#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
std::atomic<std::uintptr_t> expected_audio_data{0};
std::atomic<std::uintptr_t> expected_video_data{0};
std::atomic<std::uintptr_t> expected_byte_data{0};

muxiva_str_v1 text(const char* value) { return {value, std::char_traits<char>::length(value)}; }

muxiva_frame_view_v1 header(uint32_t type, uint64_t sequence) {
  muxiva_frame_view_v1 frame{};
  frame.header.abi_version = MUXIVA_ABI_VERSION_V1;
  frame.header.struct_size = sizeof(frame.header);
  frame.header.frame_type = type;
  frame.header.clock_kind = MUXIVA_CLOCK_MONOTONIC;
  frame.header.sequence_id = sequence;
  frame.header.frame_id = text("cpp-multimodal");
  frame.header.clock_domain_id = text("cpp.monotonic");
  frame.header.stream_id = text("cpp-stream");
  frame.header.trace_id = text("cpp-trace");
  return frame;
}

class Source final : public muxiva::MultimodalGraphNode {
 public:
  explicit Source(const std::string& config) {
    if (config.find("demo") == std::string::npos) throw std::runtime_error("missing config");
  }
  void on_process(const muxiva_frame_view_v1*, muxiva::GraphNodeContext& context) override {
    std::vector<uint8_t> audio_payload{0, 0};
    auto audio = header(MUXIVA_FRAME_AUDIO, 1);
    audio.payload.audio.sample_rate_hz = 8000;
    audio.payload.audio.channels = 1;
    audio.payload.audio.sample_format = MUXIVA_PCM_I16LE;
    audio.payload.audio.layout = MUXIVA_AUDIO_INTERLEAVED;
    audio.payload.audio.samples_per_channel = 1;
    audio.payload.audio.bytes = {audio_payload.data(), audio_payload.size()};
    expected_audio_data.store(reinterpret_cast<std::uintptr_t>(audio_payload.data()),
                              std::memory_order_release);

    std::vector<uint8_t> video_payload{1, 2, 3, 4};
    auto video = header(MUXIVA_FRAME_VIDEO, 2);
    video.payload.video.width = 1; video.payload.video.height = 1;
    video.payload.video.pixel_format = MUXIVA_PIXEL_RGBA8; video.payload.video.plane_count = 1;
    video.payload.video.bytes = {video_payload.data(), video_payload.size()};
    expected_video_data.store(reinterpret_cast<std::uintptr_t>(video_payload.data()),
                              std::memory_order_release);

    std::vector<uint8_t> byte_payload{5, 6, 7};
    auto bytes = header(MUXIVA_FRAME_BYTE, 3);
    bytes.payload.bytes.bytes = {byte_payload.data(), byte_payload.size()};
    bytes.payload.bytes.media_type = text("application/octet-stream");
    expected_byte_data.store(reinterpret_cast<std::uintptr_t>(byte_payload.data()),
                             std::memory_order_release);

    auto message = header(MUXIVA_FRAME_TEXT, 4);
    message.payload.text.text = {message_.data(), message_.size()};
    context.emit_owned("audio_out",
                       muxiva::OwnedFrame(audio, std::move(audio_payload)));
    context.emit_owned("video_out",
                       muxiva::OwnedFrame(video, std::move(video_payload)));
    context.emit_owned("byte_out",
                       muxiva::OwnedFrame(bytes, std::move(byte_payload)));
    context.emit("text_out", message);
    // Safe-copy emissions must own borrowed ABI data immediately. The owned
    // media emissions above retain their original allocation across the FFI.
    message_[0] = 'X';
    context.increment_counter("source.frames", 4);
    context.set_gauge("source.last_sequence", 4);
  }
 private:
  std::string message_ = "hello";
};

class Sink final : public muxiva::MultimodalGraphNode {
 public:
  void on_process(const muxiva_frame_view_v1* input,
                  muxiva::GraphNodeContext& context) override {
    if (input == nullptr || context.input_port() != "in") {
      throw std::runtime_error("invalid sink input");
    }
    switch (input->header.frame_type) {
      case MUXIVA_FRAME_AUDIO:
        if (input->payload.audio.bytes.len != 2 || input->payload.audio.bytes.data[0] != 0)
          throw std::runtime_error("audio emission did not retain its payload");
        if (reinterpret_cast<std::uintptr_t>(input->payload.audio.bytes.data) !=
            expected_audio_data.load(std::memory_order_acquire))
          throw std::runtime_error("audio owned emission was copied");
        break;
      case MUXIVA_FRAME_VIDEO:
        if (input->payload.video.bytes.len != 4 || input->payload.video.bytes.data[0] != 1)
          throw std::runtime_error("video emission did not retain its payload");
        if (reinterpret_cast<std::uintptr_t>(input->payload.video.bytes.data) !=
            expected_video_data.load(std::memory_order_acquire))
          throw std::runtime_error("video owned emission was copied");
        break;
      case MUXIVA_FRAME_BYTE:
        if (input->payload.bytes.bytes.len != 3 || input->payload.bytes.bytes.data[0] != 5)
          throw std::runtime_error("byte emission did not retain its payload");
        if (reinterpret_cast<std::uintptr_t>(input->payload.bytes.bytes.data) !=
            expected_byte_data.load(std::memory_order_acquire))
          throw std::runtime_error("byte owned emission was copied");
        break;
      case MUXIVA_FRAME_TEXT:
        if (std::string_view(input->payload.text.text.data,
                             input->payload.text.text.len) != "hello")
          throw std::runtime_error("text emission did not retain its payload");
        break;
      default:
        throw std::runtime_error("unexpected sink frame type");
    }
  }
};
}  // namespace

int main() {
  const std::string graph = R"({
    "version":"muxiva.graph/v1","graph_id":"cpp-multimodal",
    "nodes":[
      {"id":"source","node_type":"example.cpp.multimodal-source","language":"cpp","factory_version":"1.0.0","node_config":{"label":"demo"}},
      {"id":"audio-sink","node_type":"example.cpp.audio-sink","language":"cpp","factory_version":"1.0.0","node_config":{}},
      {"id":"video-sink","node_type":"example.cpp.video-sink","language":"cpp","factory_version":"1.0.0","node_config":{}},
      {"id":"byte-sink","node_type":"example.cpp.byte-sink","language":"cpp","factory_version":"1.0.0","node_config":{}},
      {"id":"text-sink","node_type":"example.cpp.text-sink","language":"cpp","factory_version":"1.0.0","node_config":{}}
    ],"edges":[
      {"id":"audio","from":{"node_id":"source","port":"audio_out"},"to":{"node_id":"audio-sink","port":"in"},"frame_type":"audio","queue_policy":{"capacity":8,"overflow":"block"}},
      {"id":"video","from":{"node_id":"source","port":"video_out"},"to":{"node_id":"video-sink","port":"in"},"frame_type":"video","queue_policy":{"capacity":8,"overflow":"block"}},
      {"id":"byte","from":{"node_id":"source","port":"byte_out"},"to":{"node_id":"byte-sink","port":"in"},"frame_type":"byte","queue_policy":{"capacity":8,"overflow":"block"}},
      {"id":"text","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"text-sink","port":"in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}}
    ]})";
  const std::string source_ports = R"([{"name":"audio_out","direction":"output","frameType":"audio"},{"name":"video_out","direction":"output","frameType":"video"},{"name":"byte_out","direction":"output","frameType":"byte"},{"name":"text_out","direction":"output","frameType":"text"}])";
  muxiva::Error error;
  muxiva::Runtime runtime(error);
  std::vector<muxiva::MultimodalGraphNodeFactory> factories;
  factories.push_back(muxiva::MultimodalGraphNodeFactory::make<Source>("example.cpp.multimodal-source", MUXIVA_NODE_SOURCE, source_ports, R"({"type":"object"})"));
  for (const std::string type : {"audio", "video", "byte", "text"}) {
    factories.push_back(muxiva::MultimodalGraphNodeFactory::make<Sink>(
        "example.cpp." + type + "-sink", MUXIVA_NODE_SINK,
        "[{\"name\":\"in\",\"direction\":\"input\",\"frameType\":\"" + type + "\"}]"));
  }
  uint32_t workers = 0;
  if (runtime.run_multimodal_graph(graph, factories, workers, error) != MUXIVA_STATUS_OK) {
    std::cerr << error.code() << ": " << error.message() << '\n'; return 1;
  }
  std::cout << "C++ multimodal Graph completed with " << workers << " workers\n";
  return workers == 5 ? 0 : 1;
}
