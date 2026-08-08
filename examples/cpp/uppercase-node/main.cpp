#include <muxiva/muxiva.hpp>

#include <algorithm>
#include <cctype>
#include <iostream>
#include <string>
#include <vector>

namespace {
int process_count = 0;

muxiva_str_v1 borrow(const std::string& value) {
  return {value.data(), value.size()};
}

class UppercaseNode final : public muxiva::TransformNode {
 public:
  void on_process(const muxiva_frame_view_v1& input,
                  muxiva_frame_view_v1& output) override {
    ++process_count;
    const auto text = input.payload.text.text;
    value_.assign(text.data, text.len);
    std::transform(value_.begin(), value_.end(), value_.begin(),
                   [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
    output = input;
    output.payload.text.text = borrow(value_);
  }

 private:
  std::string value_;
};
}  // namespace

int main() {
  muxiva::Error error;
  muxiva::Runtime runtime(error);
  auto node = muxiva::Node::make<UppercaseNode>(error);
  const muxiva::TextFrame input("hello muxiva", 1);
  std::string output;
  if (runtime.run_text(node, input, output, error) != MUXIVA_STATUS_OK) {
    std::cerr << error.code() << ": " << error.message() << '\n';
    return 1;
  }
  std::cout << output << '\n';
  if (output != "HELLO MUXIVA") return 2;

  const std::string graph = R"json({
    "version":"muxiva.graph/v1",
    "graph_id":"cpp-registered",
    "nodes":[
      {"id":"source","node_type":"builtin.text_source","language":"rust","factory_version":"1.0.0","node_config":{"text":"hello"}},
      {"id":"upper","node_type":"example.cpp.uppercase","language":"cpp","factory_version":"1.0.0","node_config":{}},
      {"id":"sink","node_type":"builtin.text_sink","language":"rust","factory_version":"1.0.0","node_config":{}}
    ],
    "edges":[
      {"id":"source-upper","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"upper","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}},
      {"id":"upper-sink","from":{"node_id":"upper","port":"text_out"},"to":{"node_id":"sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}}
    ]
  })json";
  const auto before_graph = process_count;
  const std::vector<muxiva::GraphNodeFactory> factories{
      muxiva::GraphNodeFactory::make<UppercaseNode>("example.cpp.uppercase")};
  uint32_t workers = 0;
  if (runtime.run_graph(graph, factories, workers, error) != MUXIVA_STATUS_OK) {
    std::cerr << error.code() << ": " << error.message() << '\n';
    return 3;
  }
  return workers == 3 && process_count == before_graph + 1 ? 0 : 4;
}
