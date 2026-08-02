#ifndef VOXA_VOXA_HPP
#define VOXA_VOXA_HPP

#include "voxa.h"

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <cstring>
#include <functional>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace voxa {

class Error final {
 public:
  Error() noexcept { reset(); }
  void reset() noexcept {
    std::memset(&value_, 0, sizeof(value_));
    value_.abi_version = VOXA_ABI_VERSION_V1;
    value_.struct_size = sizeof(value_);
  }
  voxa_error_v1* out() noexcept { return &value_; }
  voxa_status_v1 status() const noexcept { return value_.status; }
  std::string code() const { return value_.code; }
  std::string message() const { return value_.message; }

 private:
  voxa_error_v1 value_{};
};

/// An owned text frame whose borrowed ABI view remains valid until mutation or
/// destruction of this object.
class TextFrame final {
 public:
  explicit TextFrame(std::string text, uint64_t sequence = 0,
                     int64_t timestamp_ns = 0)
      : text_(std::move(text)), sequence_(sequence), timestamp_ns_(timestamp_ns) {}

  const std::string& text() const noexcept { return text_; }
  uint64_t sequence() const noexcept { return sequence_; }

  voxa_frame_view_v1 view() const noexcept {
    voxa_frame_view_v1 frame{};
    frame.header.abi_version = VOXA_ABI_VERSION_V1;
    frame.header.struct_size = sizeof(frame.header);
    frame.header.frame_type = VOXA_FRAME_TEXT;
    frame.header.clock_kind = VOXA_CLOCK_MONOTONIC;
    frame.header.timestamp_ns = timestamp_ns_;
    frame.header.sequence_id = sequence_;
    frame.header.frame_id = borrow(frame_id_);
    frame.header.clock_domain_id = borrow(clock_domain_);
    frame.header.stream_id = borrow(stream_id_);
    frame.header.trace_id = borrow(trace_id_);
    frame.payload.text.text = borrow(text_);
    return frame;
  }

 private:
  static voxa_str_v1 borrow(const std::string& value) noexcept {
    return {value.data(), value.size()};
  }

  std::string text_;
  uint64_t sequence_;
  int64_t timestamp_ns_;
  std::string frame_id_ = "cpp-input";
  std::string clock_domain_ = "cpp.monotonic";
  std::string stream_id_ = "cpp";
  std::string trace_id_ = "cpp-trace";
};

class TransformNode {
 public:
  virtual ~TransformNode() = default;
  virtual void on_prepare() {}
  virtual void on_process(const voxa_frame_view_v1& input,
                          voxa_frame_view_v1& output) = 0;
  virtual void on_signal(const voxa_frame_view_v1&) {}
  virtual void on_finish() {}
  virtual void on_abort(const voxa_abort_reason_v1&) noexcept {}
};

namespace detail {
inline void write_exception(voxa_error_v1* error) noexcept {
  if (error == nullptr) return;
  error->status = VOXA_STATUS_FOREIGN_EXCEPTION;
  error->category = VOXA_ERROR_CATEGORY_FOREIGN_EXCEPTION;
  std::strncpy(error->code, "VOXA-FFI-CPP-EXCEPTION", sizeof(error->code) - 1);
  std::strncpy(error->message, "C++ exception caught by Voxa trampoline",
               sizeof(error->message) - 1);
}
inline voxa_status_v1 prepare(void* data, voxa_error_v1* error) noexcept {
  try {
    static_cast<TransformNode*>(data)->on_prepare();
    return VOXA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return VOXA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline voxa_status_v1 process(void* data, const voxa_frame_view_v1* input,
                              voxa_frame_view_v1* output,
                              voxa_error_v1* error) noexcept {
  try {
    if (input == nullptr || output == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
    static_cast<TransformNode*>(data)->on_process(*input, *output);
    return VOXA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return VOXA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline voxa_status_v1 signal(void* data, const voxa_frame_view_v1* input,
                             voxa_error_v1* error) noexcept {
  try {
    if (input == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
    static_cast<TransformNode*>(data)->on_signal(*input);
    return VOXA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return VOXA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline voxa_status_v1 finish(void* data, voxa_error_v1* error) noexcept {
  try {
    static_cast<TransformNode*>(data)->on_finish();
    return VOXA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return VOXA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline void abort(void* data, const voxa_abort_reason_v1* reason) noexcept {
  try {
    if (reason != nullptr) static_cast<TransformNode*>(data)->on_abort(*reason);
  } catch (...) {
  }
}
inline void destroy(void* data) noexcept {
  try {
    delete static_cast<TransformNode*>(data);
  } catch (...) {
  }
}
inline voxa_node_vtable_v1 node_vtable(TransformNode* implementation) noexcept {
  voxa_node_vtable_v1 table{};
  table.abi_version = VOXA_ABI_VERSION_V1;
  table.struct_size = sizeof(table);
  table.user_data = implementation;
  table.on_prepare = prepare;
  table.on_process = process;
  table.on_signal = signal;
  table.on_finish = finish;
  table.on_abort = abort;
  table.destroy = destroy;
  return table;
}
}  // namespace detail

class Node final {
 public:
  Node() noexcept = default;
  explicit Node(TransformNode* implementation, Error& error) {
    auto table = detail::node_vtable(implementation);
    const auto status = voxa_node_create_v1(&table, &handle_, error.out());
    if (status != VOXA_STATUS_OK) {
      delete implementation;
      throw std::runtime_error(error.message());
    }
    open_ = true;
  }
  Node(const Node&) = delete;
  Node& operator=(const Node&) = delete;
  Node(Node&& other) noexcept : handle_(other.handle_), open_(other.open_) {
    other.open_ = false;
  }
  Node& operator=(Node&& other) noexcept {
    if (this != &other) {
      close();
      handle_ = other.handle_;
      open_ = other.open_;
      other.open_ = false;
    }
    return *this;
  }
  ~Node() noexcept { close(); }
  template <typename T, typename... Args>
  static Node make(Error& error, Args&&... args) {
    static_assert(std::is_base_of<TransformNode, T>::value,
                  "T must derive from voxa::TransformNode");
    return Node(new T(std::forward<Args>(args)...), error);
  }
  voxa_status_v1 close() noexcept {
    if (!open_) return VOXA_STATUS_CLOSED;
    const auto status = voxa_node_release_v1(handle_);
    if (status == VOXA_STATUS_OK || status == VOXA_STATUS_CLOSED) open_ = false;
    return status;
  }
  voxa_node_v1 get() const noexcept { return handle_; }

 private:
  voxa_node_v1 handle_{};
  bool open_ = false;
};

class GraphNodeFactory final {
 public:
  using Creator = std::function<TransformNode*()>;

  GraphNodeFactory(std::string node_type, Creator creator,
                   std::string version = "1.0.0",
                   std::string input_port = "text_in",
                   std::string output_port = "text_out")
      : node_type_(std::move(node_type)),
        version_(std::move(version)),
        input_port_(std::move(input_port)),
        output_port_(std::move(output_port)),
        creator_(std::make_shared<Creator>(std::move(creator))) {}

  template <typename T>
  static GraphNodeFactory make(std::string node_type) {
    static_assert(std::is_base_of<TransformNode, T>::value,
                  "T must derive from voxa::TransformNode");
    return GraphNodeFactory(std::move(node_type), []() -> TransformNode* {
      return new T();
    });
  }

  voxa_node_factory_v1 view() const noexcept {
    voxa_node_factory_v1 result{};
    result.abi_version = VOXA_ABI_VERSION_V1;
    result.struct_size = sizeof(result);
    result.node_type = borrow(node_type_);
    result.version = borrow(version_);
    result.input_port = borrow(input_port_);
    result.output_port = borrow(output_port_);
    result.user_data = creator_.get();
    result.create = create;
    return result;
  }

 private:
  static voxa_str_v1 borrow(const std::string& value) noexcept {
    return {value.data(), value.size()};
  }
  static voxa_status_v1 create(void* data, voxa_str_v1,
                               voxa_node_vtable_v1* output,
                               voxa_error_v1* error) noexcept {
    try {
      if (data == nullptr || output == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
      auto* implementation = (*static_cast<Creator*>(data))();
      if (implementation == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
      *output = detail::node_vtable(implementation);
      return VOXA_STATUS_OK;
    } catch (...) {
      detail::write_exception(error);
      return VOXA_STATUS_FOREIGN_EXCEPTION;
    }
  }

  std::string node_type_;
  std::string version_;
  std::string input_port_;
  std::string output_port_;
  std::shared_ptr<Creator> creator_;
};

struct GraphEmission {
  std::string output_port;
  voxa_frame_view_v1 frame{};
};

class GraphNodeContext {
 public:
  explicit GraphNodeContext(std::string_view input_port) : input_port_(input_port) {}
  std::string_view input_port() const noexcept { return input_port_; }
  void emit(std::string output_port, voxa_frame_view_v1 frame) {
    emissions_.push_back({std::move(output_port), frame});
  }
  void schedule_next_tick(std::chrono::nanoseconds delay) {
    next_source_tick_ns_ = delay.count() > 0
        ? static_cast<std::uint64_t>(delay.count()) : 1;
  }
  std::vector<GraphEmission> take_emissions() { return std::move(emissions_); }
  std::uint64_t take_next_source_tick_ns() noexcept {
    const auto value = next_source_tick_ns_;
    next_source_tick_ns_ = 0;
    return value;
  }

 private:
  std::string_view input_port_;
  std::vector<GraphEmission> emissions_;
  std::uint64_t next_source_tick_ns_ = 0;
};

class MultimodalGraphNode {
 public:
  virtual ~MultimodalGraphNode() = default;
  virtual void on_prepare() {}
  virtual void on_process(const voxa_frame_view_v1* input,
                          GraphNodeContext& context) {
    for (auto& emission : on_process(input, context.input_port())) {
      context.emit(std::move(emission.output_port), emission.frame);
    }
  }
  // V1 source compatibility. New Nodes should override the context form.
  virtual std::vector<GraphEmission> on_process(
      const voxa_frame_view_v1*, std::string_view) { return {}; }
  virtual void on_signal(const voxa_frame_view_v1&) {}
  virtual void on_finish() {}
  virtual void on_abort(const voxa_abort_reason_v1&) noexcept {}
};

namespace detail {
struct MultimodalNodeBox {
  explicit MultimodalNodeBox(MultimodalGraphNode* value) : implementation(value) {}
  std::unique_ptr<MultimodalGraphNode> implementation;
  std::vector<GraphEmission> emissions;
  std::vector<voxa_named_frame_v1> views;
  std::uint64_t next_source_tick_ns = 0;
};
inline voxa_status_v1 multimodal_prepare(void* data, voxa_error_v1* error) noexcept {
  try { static_cast<MultimodalNodeBox*>(data)->implementation->on_prepare(); return VOXA_STATUS_OK; }
  catch (...) { write_exception(error); return VOXA_STATUS_FOREIGN_EXCEPTION; }
}
inline voxa_status_v1 multimodal_process(
    void* data, const voxa_frame_view_v1* input, voxa_str_v1 input_port,
    const voxa_named_frame_v1** output, size_t* output_count,
    voxa_error_v1* error) noexcept {
  try {
    if (data == nullptr || output == nullptr || output_count == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
    auto* box = static_cast<MultimodalNodeBox*>(data);
    const std::string_view port(input_port.data == nullptr ? "" : input_port.data,
                                input_port.data == nullptr ? 0 : input_port.len);
    GraphNodeContext context(port);
    box->implementation->on_process(input, context);
    box->emissions = context.take_emissions();
    box->next_source_tick_ns = context.take_next_source_tick_ns();
    box->views.clear();
    box->views.reserve(box->emissions.size());
    for (const auto& emission : box->emissions) {
      box->views.push_back({{emission.output_port.data(), emission.output_port.size()}, emission.frame});
    }
    *output = box->views.empty() ? nullptr : box->views.data();
    *output_count = box->views.size();
    return VOXA_STATUS_OK;
  } catch (...) { write_exception(error); return VOXA_STATUS_FOREIGN_EXCEPTION; }
}
inline voxa_status_v1 multimodal_finish(void* data, voxa_error_v1* error) noexcept {
  try { static_cast<MultimodalNodeBox*>(data)->implementation->on_finish(); return VOXA_STATUS_OK; }
  catch (...) { write_exception(error); return VOXA_STATUS_FOREIGN_EXCEPTION; }
}
inline voxa_status_v1 multimodal_signal(
    void* data, const voxa_frame_view_v1* signal, voxa_error_v1* error) noexcept {
  try {
    if (data == nullptr || signal == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
    static_cast<MultimodalNodeBox*>(data)->implementation->on_signal(*signal);
    return VOXA_STATUS_OK;
  } catch (...) { write_exception(error); return VOXA_STATUS_FOREIGN_EXCEPTION; }
}
inline void multimodal_abort(void* data, const voxa_abort_reason_v1* reason) noexcept {
  try { if (reason != nullptr) static_cast<MultimodalNodeBox*>(data)->implementation->on_abort(*reason); } catch (...) {}
}
inline void multimodal_destroy(void* data) noexcept {
  try { delete static_cast<MultimodalNodeBox*>(data); } catch (...) {}
}
inline std::uint64_t multimodal_take_next_source_tick(void* data) noexcept {
  if (data == nullptr) return 0;
  auto* box = static_cast<MultimodalNodeBox*>(data);
  const auto value = box->next_source_tick_ns;
  box->next_source_tick_ns = 0;
  return value;
}
inline voxa_graph_node_vtable_v1 multimodal_vtable(MultimodalGraphNode* implementation) {
  voxa_graph_node_vtable_v1 table{};
  table.abi_version = VOXA_ABI_VERSION_V1;
  table.struct_size = sizeof(table);
  table.user_data = new MultimodalNodeBox(implementation);
  table.on_prepare = multimodal_prepare;
  table.on_process = multimodal_process;
  table.on_signal = multimodal_signal;
  table.on_finish = multimodal_finish;
  table.on_abort = multimodal_abort;
  table.destroy = multimodal_destroy;
  table.take_next_source_tick_ns = multimodal_take_next_source_tick;
  return table;
}
}  // namespace detail

class MultimodalGraphNodeFactory final {
 public:
  using Creator = std::function<MultimodalGraphNode*(const std::string&)>;
  MultimodalGraphNodeFactory(std::string node_type, uint32_t kind,
                             std::string ports_json, Creator creator,
                             std::string config_schema_json = "{}",
                             std::string version = "1.0.0")
      : node_type_(std::move(node_type)), version_(std::move(version)), kind_(kind),
        ports_json_(std::move(ports_json)), config_schema_json_(std::move(config_schema_json)),
        creator_(std::make_shared<Creator>(std::move(creator))) {}

  template <typename T>
  static MultimodalGraphNodeFactory make(std::string node_type, uint32_t kind,
                                          std::string ports_json,
                                          std::string config_schema_json = "{}",
                                          std::string version = "1.0.0") {
    static_assert(std::is_base_of<MultimodalGraphNode, T>::value,
                  "T must derive from voxa::MultimodalGraphNode");
    return MultimodalGraphNodeFactory(
        std::move(node_type), kind, std::move(ports_json),
        [](const std::string& config) -> MultimodalGraphNode* {
          if constexpr (std::is_constructible<T, const std::string&>::value) return new T(config);
          else return new T();
        }, std::move(config_schema_json), std::move(version));
  }

  voxa_multimodal_node_factory_v1 view() const noexcept {
    voxa_multimodal_node_factory_v1 result{};
    result.abi_version = VOXA_ABI_VERSION_V1; result.struct_size = sizeof(result);
    result.node_type = borrow(node_type_); result.version = borrow(version_); result.kind = kind_;
    result.ports_json = borrow(ports_json_); result.config_schema_json = borrow(config_schema_json_);
    result.user_data = creator_.get(); result.create = create;
    return result;
  }

 private:
  static voxa_str_v1 borrow(const std::string& value) noexcept { return {value.data(), value.size()}; }
  static voxa_status_v1 create(void* data, voxa_str_v1, voxa_str_v1 config,
                               voxa_graph_node_vtable_v1* output,
                               voxa_error_v1* error) noexcept {
    try {
      if (data == nullptr || output == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
      const std::string config_value(config.data == nullptr ? "" : config.data,
                                     config.data == nullptr ? 0 : config.len);
      auto* implementation = (*static_cast<Creator*>(data))(config_value);
      if (implementation == nullptr) return VOXA_STATUS_INVALID_ARGUMENT;
      const auto caller_size = static_cast<std::size_t>(output->struct_size);
      constexpr auto legacy_size = offsetof(voxa_graph_node_vtable_v1,
                                             take_next_source_tick_ns);
      if (caller_size < legacy_size) {
        delete implementation;
        return VOXA_STATUS_INVALID_ARGUMENT;
      }
      auto table = detail::multimodal_vtable(implementation);
      const auto copied_size = std::min(caller_size, sizeof(table));
      std::memcpy(output, &table, copied_size);
      output->struct_size = static_cast<std::uint32_t>(copied_size);
      return VOXA_STATUS_OK;
    } catch (...) { detail::write_exception(error); return VOXA_STATUS_FOREIGN_EXCEPTION; }
  }
  std::string node_type_, version_;
  uint32_t kind_;
  std::string ports_json_, config_schema_json_;
  std::shared_ptr<Creator> creator_;
};

class Runtime final {
 public:
  explicit Runtime(Error& error) {
    const auto status = voxa_runtime_create_v1(&handle_, error.out());
    if (status != VOXA_STATUS_OK) throw std::runtime_error(error.message());
    open_ = true;
  }
  Runtime(const Runtime&) = delete;
  Runtime& operator=(const Runtime&) = delete;
  Runtime(Runtime&& other) noexcept : handle_(other.handle_), open_(other.open_) {
    other.open_ = false;
  }
  ~Runtime() noexcept { close(); }
  voxa_status_v1 close() noexcept {
    if (!open_) return VOXA_STATUS_CLOSED;
    const auto status = voxa_runtime_release_v1(handle_);
    if (status == VOXA_STATUS_OK || status == VOXA_STATUS_CLOSED) open_ = false;
    return status;
  }
  voxa_status_v1 run_text(const Node& node, const voxa_frame_view_v1& input,
                          std::string& output, Error& error) const {
    char bytes[4096]{};
    size_t length = 0;
    const auto status = voxa_runtime_run_text_v1(
        handle_, node.get(), &input, bytes, sizeof(bytes), &length, error.out());
    if (status == VOXA_STATUS_OK) output.assign(bytes, length);
    return status;
  }
  voxa_status_v1 run_text(const Node& node, const TextFrame& input,
                          std::string& output, Error& error) const {
    const auto borrowed = input.view();
    return run_text(node, borrowed, output, error);
  }
  voxa_status_v1 run_graph(const std::string& graph_json,
                           const std::vector<GraphNodeFactory>& factories,
                           uint32_t& worker_total, Error& error,
                           uint64_t timeout_ms = 30000) const {
    std::vector<voxa_node_factory_v1> views;
    views.reserve(factories.size());
    for (const auto& factory : factories) views.push_back(factory.view());
    voxa_graph_run_summary_v1 summary{};
    summary.abi_version = VOXA_ABI_VERSION_V1;
    summary.struct_size = sizeof(summary);
    const voxa_str_v1 json{graph_json.data(), graph_json.size()};
    const auto status = voxa_runtime_run_graph_v1(
        handle_, json, views.data(), views.size(), timeout_ms, &summary,
        error.out());
    if (status == VOXA_STATUS_OK) worker_total = summary.worker_total;
    return status;
  }
  voxa_status_v1 run_multimodal_graph(
      const std::string& graph_json,
      const std::vector<MultimodalGraphNodeFactory>& factories,
      uint32_t& worker_total, Error& error,
      uint64_t timeout_ms = 30000) const {
    std::vector<voxa_multimodal_node_factory_v1> views;
    views.reserve(factories.size());
    for (const auto& factory : factories) views.push_back(factory.view());
    voxa_graph_run_summary_v1 summary{};
    summary.abi_version = VOXA_ABI_VERSION_V1; summary.struct_size = sizeof(summary);
    const voxa_str_v1 json{graph_json.data(), graph_json.size()};
    const auto status = voxa_runtime_run_multimodal_graph_v1(
        handle_, json, views.data(), views.size(), timeout_ms, &summary, error.out());
    if (status == VOXA_STATUS_OK) worker_total = summary.worker_total;
    return status;
  }

 private:
  voxa_runtime_v1 handle_{};
  bool open_ = false;
};

}  // namespace voxa
#endif
