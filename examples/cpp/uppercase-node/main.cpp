#include <voxa/voxa.hpp>

#include <algorithm>
#include <cctype>
#include <iostream>
#include <string>

namespace {
voxa_str_v1 borrow(const std::string& value) {
  return {value.data(), value.size()};
}

class UppercaseNode final : public voxa::TransformNode {
 public:
  void on_process(const voxa_frame_view_v1& input,
                  voxa_frame_view_v1& output) override {
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
  voxa::Error error;
  voxa::Runtime runtime(error);
  auto node = voxa::Node::make<UppercaseNode>(error);
  const voxa::TextFrame input("hello voxa", 1);
  std::string output;
  if (runtime.run_text(node, input, output, error) != VOXA_STATUS_OK) {
    std::cerr << error.code() << ": " << error.message() << '\n';
    return 1;
  }
  std::cout << output << '\n';
  return output == "HELLO VOXA" ? 0 : 2;
}
