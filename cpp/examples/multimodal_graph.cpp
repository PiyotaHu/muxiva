#include <muxiva/muxiva.hpp>

#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
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
    auto audio = header(MUXIVA_FRAME_AUDIO, 1);
    audio.payload.audio.sample_rate_hz = 8000;
    audio.payload.audio.channels = 1;
    audio.payload.audio.sample_format = MUXIVA_PCM_I16LE;
    audio.payload.audio.layout = MUXIVA_AUDIO_INTERLEAVED;
    audio.payload.audio.samples_per_channel = 1;
    audio.payload.audio.bytes = {audio_.data(), audio_.size()};

    auto video = header(MUXIVA_FRAME_VIDEO, 2);
    video.payload.video.width = 1; video.payload.video.height = 1;
    video.payload.video.pixel_format = MUXIVA_PIXEL_RGBA8; video.payload.video.plane_count = 1;
    video.payload.video.bytes = {video_.data(), video_.size()};

    auto bytes = header(MUXIVA_FRAME_BYTE, 3);
    bytes.payload.bytes.bytes = {bytes_.data(), bytes_.size()};
    bytes.payload.bytes.media_type = text("application/octet-stream");

    auto message = header(MUXIVA_FRAME_TEXT, 4);
    message.payload.text.text = {message_.data(), message_.size()};
    context.emit("audio_out", audio);
    context.emit("video_out", video);
    context.emit("byte_out", bytes);
    context.emit("text_out", message);
  }
 private:
  std::vector<uint8_t> audio_{0, 0};
  std::vector<uint8_t> video_{1, 2, 3, 4};
  std::vector<uint8_t> bytes_{5, 6, 7};
  std::string message_ = "hello";
};

class Sink final : public muxiva::MultimodalGraphNode {
 public:
  void on_process(const muxiva_frame_view_v1* input,
                  muxiva::GraphNodeContext& context) override {
    if (input == nullptr || context.input_port() != "in") {
      throw std::runtime_error("invalid sink input");
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
