#include "voxa/voxa.hpp"

#include <algorithm>
#include <cassert>
#include <cctype>
#include <cstring>
#include <stdexcept>
#include <string>

namespace {
int abort_count = 0;

voxa_str_v1 view(const std::string& value) {
  return {value.data(), value.size()};
}

class Uppercase final : public voxa::TransformNode {
 public:
  void on_prepare() override { prepared_ = true; }
  void on_process(const voxa_frame_view_v1& input,
                  voxa_frame_view_v1& output) override {
    assert(prepared_);
    const auto text = input.payload.text.text;
    uppercase_.assign(text.data, text.len);
    std::transform(uppercase_.begin(), uppercase_.end(), uppercase_.begin(),
                   [](unsigned char c) { return static_cast<char>(std::toupper(c)); });
    frame_id_ = "cpp-uppercase-1";
    output.header = input.header;
    output.header.abi_version = VOXA_ABI_VERSION_V1;
    output.header.struct_size = sizeof(output.header);
    output.header.frame_id = view(frame_id_);
    output.header.sequence_id += 1;
    output.payload.text = {};
    output.payload.text.text = view(uppercase_);
  }
  void on_finish() override { prepared_ = false; }
  void on_abort(const voxa_abort_reason_v1&) noexcept override { aborted_ = true; }

 private:
  bool prepared_ = false;
  bool aborted_ = false;
  std::string uppercase_;
  std::string frame_id_;
};

class Throwing final : public voxa::TransformNode {
 public:
  void on_process(const voxa_frame_view_v1&, voxa_frame_view_v1&) override {
    throw std::runtime_error("must not cross the ABI");
  }
  void on_abort(const voxa_abort_reason_v1&) noexcept override { ++abort_count; }
};

voxa_frame_view_v1 text_frame(const std::string& text) {
  static const std::string frame_id = "cpp-input-1";
  static const std::string clock = "cpp.media";
  static const std::string stream = "cpp-stream";
  static const std::string trace = "cpp-trace";
  voxa_frame_view_v1 frame{};
  frame.header.abi_version = VOXA_ABI_VERSION_V1;
  frame.header.struct_size = sizeof(frame.header);
  frame.header.frame_type = VOXA_FRAME_TEXT;
  frame.header.clock_kind = VOXA_CLOCK_MEDIA_RELATIVE;
  frame.header.sequence_id = 1;
  frame.header.frame_id = view(frame_id);
  frame.header.clock_domain_id = view(clock);
  frame.header.stream_id = view(stream);
  frame.header.trace_id = view(trace);
  frame.payload.text.text = view(text);
  return frame;
}
}  // namespace

int main() {
  assert(voxa_abi_version_v1() == VOXA_ABI_VERSION_V1);
  assert((voxa_capabilities_v1() & VOXA_CAP_RETAIN_RELEASE) == 0);
  voxa::Error error;
  voxa::Runtime runtime(error);
  auto node = voxa::Node::make<Uppercase>(error);
  std::string input = "Hello, Voxa";
  auto frame = text_frame(input);
  std::string output;
  assert(runtime.run_text(node, frame, output, error) == VOXA_STATUS_OK);
  input.assign(input.size(), 'x');
  assert(output == "HELLO, VOXA");
  voxa::TextFrame owned_input("Owned C++ frame", 2);
  assert(runtime.run_text(node, owned_input, output, error) == VOXA_STATUS_OK);
  assert(output == "OWNED C++ FRAME");
  assert(node.close() == VOXA_STATUS_OK);
  assert(node.close() == VOXA_STATUS_CLOSED);
  voxa::Node throwing(new Throwing(), error);
  assert(runtime.run_text(throwing, frame, output, error) ==
         VOXA_STATUS_FOREIGN_EXCEPTION);
  assert(abort_count == 1);
  assert(throwing.close() == VOXA_STATUS_OK);
  assert(runtime.close() == VOXA_STATUS_OK);
  return 0;
}
