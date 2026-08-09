#ifndef MUXIVA_MUXIVA_HPP
#define MUXIVA_MUXIVA_HPP

#include "muxiva.h"

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <functional>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace muxiva {

class Error final {
 public:
  Error() noexcept { reset(); }
  void reset() noexcept {
    std::memset(&value_, 0, sizeof(value_));
    value_.abi_version = MUXIVA_ABI_VERSION_V1;
    value_.struct_size = sizeof(value_);
  }
  muxiva_error_v1* out() noexcept { return &value_; }
  muxiva_status_v1 status() const noexcept { return value_.status; }
  std::string code() const { return value_.code; }
  std::string message() const { return value_.message; }

 private:
  muxiva_error_v1 value_{};
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

  muxiva_frame_view_v1 view() const noexcept {
    muxiva_frame_view_v1 frame{};
    frame.header.abi_version = MUXIVA_ABI_VERSION_V1;
    frame.header.struct_size = sizeof(frame.header);
    frame.header.frame_type = MUXIVA_FRAME_TEXT;
    frame.header.clock_kind = MUXIVA_CLOCK_MONOTONIC;
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
  static muxiva_str_v1 borrow(const std::string& value) noexcept {
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
  virtual void on_process(const muxiva_frame_view_v1& input,
                          muxiva_frame_view_v1& output) = 0;
  virtual void on_signal(const muxiva_frame_view_v1&) {}
  virtual void on_finish() {}
  virtual void on_abort(const muxiva_abort_reason_v1&) noexcept {}
};

namespace detail {
inline void write_exception(muxiva_error_v1* error) noexcept {
  if (error == nullptr) return;
  error->status = MUXIVA_STATUS_FOREIGN_EXCEPTION;
  error->category = MUXIVA_ERROR_CATEGORY_FOREIGN_EXCEPTION;
  std::strncpy(error->code, "MUXIVA-FFI-CPP-EXCEPTION", sizeof(error->code) - 1);
  std::strncpy(error->message, "C++ exception caught by Muxiva trampoline",
               sizeof(error->message) - 1);
}
inline muxiva_status_v1 prepare(void* data, muxiva_error_v1* error) noexcept {
  try {
    static_cast<TransformNode*>(data)->on_prepare();
    return MUXIVA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return MUXIVA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline muxiva_status_v1 process(void* data, const muxiva_frame_view_v1* input,
                              muxiva_frame_view_v1* output,
                              muxiva_error_v1* error) noexcept {
  try {
    if (input == nullptr || output == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
    static_cast<TransformNode*>(data)->on_process(*input, *output);
    return MUXIVA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return MUXIVA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline muxiva_status_v1 signal(void* data, const muxiva_frame_view_v1* input,
                             muxiva_error_v1* error) noexcept {
  try {
    if (input == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
    static_cast<TransformNode*>(data)->on_signal(*input);
    return MUXIVA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return MUXIVA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline muxiva_status_v1 finish(void* data, muxiva_error_v1* error) noexcept {
  try {
    static_cast<TransformNode*>(data)->on_finish();
    return MUXIVA_STATUS_OK;
  } catch (...) {
    write_exception(error);
    return MUXIVA_STATUS_FOREIGN_EXCEPTION;
  }
}
inline void abort(void* data, const muxiva_abort_reason_v1* reason) noexcept {
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
inline muxiva_node_vtable_v1 node_vtable(TransformNode* implementation) noexcept {
  muxiva_node_vtable_v1 table{};
  table.abi_version = MUXIVA_ABI_VERSION_V1;
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
    const auto status = muxiva_node_create_v1(&table, &handle_, error.out());
    if (status != MUXIVA_STATUS_OK) {
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
                  "T must derive from muxiva::TransformNode");
    return Node(new T(std::forward<Args>(args)...), error);
  }
  muxiva_status_v1 close() noexcept {
    if (!open_) return MUXIVA_STATUS_CLOSED;
    const auto status = muxiva_node_release_v1(handle_);
    if (status == MUXIVA_STATUS_OK || status == MUXIVA_STATUS_CLOSED) open_ = false;
    return status;
  }
  muxiva_node_v1 get() const noexcept { return handle_; }

 private:
  muxiva_node_v1 handle_{};
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
                  "T must derive from muxiva::TransformNode");
    return GraphNodeFactory(std::move(node_type), []() -> TransformNode* {
      return new T();
    });
  }

  muxiva_node_factory_v1 view() const noexcept {
    muxiva_node_factory_v1 result{};
    result.abi_version = MUXIVA_ABI_VERSION_V1;
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
  static muxiva_str_v1 borrow(const std::string& value) noexcept {
    return {value.data(), value.size()};
  }
  static muxiva_status_v1 create(void* data, muxiva_str_v1,
                               muxiva_node_vtable_v1* output,
                               muxiva_error_v1* error) noexcept {
    try {
      if (data == nullptr || output == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
      auto* implementation = (*static_cast<Creator*>(data))();
      if (implementation == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
      *output = detail::node_vtable(implementation);
      return MUXIVA_STATUS_OK;
    } catch (...) {
      detail::write_exception(error);
      return MUXIVA_STATUS_FOREIGN_EXCEPTION;
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
  muxiva_frame_view_v1 frame{};

  GraphEmission(std::string port, const muxiva_frame_view_v1& value)
      : output_port(std::move(port)), frame(value) {
    own_borrowed_data();
  }

  GraphEmission(const GraphEmission& other)
      : output_port(other.output_port), frame(other.frame),
        frame_id_(other.frame_id_), clock_domain_id_(other.clock_domain_id_),
        stream_id_(other.stream_id_), trace_id_(other.trace_id_),
        primary_text_(other.primary_text_), secondary_text_(other.secondary_text_),
        payload_bytes_(other.payload_bytes_) {
    rebind();
  }

  GraphEmission& operator=(const GraphEmission& other) {
    if (this == &other) return *this;
    output_port = other.output_port;
    frame = other.frame;
    frame_id_ = other.frame_id_;
    clock_domain_id_ = other.clock_domain_id_;
    stream_id_ = other.stream_id_;
    trace_id_ = other.trace_id_;
    primary_text_ = other.primary_text_;
    secondary_text_ = other.secondary_text_;
    payload_bytes_ = other.payload_bytes_;
    rebind();
    return *this;
  }

  GraphEmission(GraphEmission&& other) noexcept
      : output_port(std::move(other.output_port)), frame(other.frame),
        frame_id_(std::move(other.frame_id_)),
        clock_domain_id_(std::move(other.clock_domain_id_)),
        stream_id_(std::move(other.stream_id_)),
        trace_id_(std::move(other.trace_id_)),
        primary_text_(std::move(other.primary_text_)),
        secondary_text_(std::move(other.secondary_text_)),
        payload_bytes_(std::move(other.payload_bytes_)) {
    rebind();
  }

  GraphEmission& operator=(GraphEmission&& other) noexcept {
    if (this == &other) return *this;
    output_port = std::move(other.output_port);
    frame = other.frame;
    frame_id_ = std::move(other.frame_id_);
    clock_domain_id_ = std::move(other.clock_domain_id_);
    stream_id_ = std::move(other.stream_id_);
    trace_id_ = std::move(other.trace_id_);
    primary_text_ = std::move(other.primary_text_);
    secondary_text_ = std::move(other.secondary_text_);
    payload_bytes_ = std::move(other.payload_bytes_);
    rebind();
    return *this;
  }

 private:
  static std::string copy_string(muxiva_str_v1 value) {
    return value.data == nullptr ? std::string{} : std::string(value.data, value.len);
  }

  static std::vector<std::uint8_t> copy_bytes(muxiva_bytes_v1 value) {
    return value.data == nullptr ? std::vector<std::uint8_t>{}
                                 : std::vector<std::uint8_t>(value.data, value.data + value.len);
  }

  static muxiva_str_v1 borrow(const std::string& value) noexcept {
    return {value.data(), value.size()};
  }

  static muxiva_bytes_v1 borrow(const std::vector<std::uint8_t>& value) noexcept {
    return {value.data(), value.size()};
  }

  void own_borrowed_data() {
    frame_id_ = copy_string(frame.header.frame_id);
    clock_domain_id_ = copy_string(frame.header.clock_domain_id);
    stream_id_ = copy_string(frame.header.stream_id);
    trace_id_ = copy_string(frame.header.trace_id);
    switch (frame.header.frame_type) {
      case MUXIVA_FRAME_AUDIO:
        payload_bytes_ = copy_bytes(frame.payload.audio.bytes);
        break;
      case MUXIVA_FRAME_VIDEO:
        payload_bytes_ = copy_bytes(frame.payload.video.bytes);
        break;
      case MUXIVA_FRAME_TEXT:
        primary_text_ = copy_string(frame.payload.text.text);
        secondary_text_ = copy_string(frame.payload.text.media_type);
        break;
      case MUXIVA_FRAME_BYTE:
        payload_bytes_ = copy_bytes(frame.payload.bytes.bytes);
        primary_text_ = copy_string(frame.payload.bytes.media_type);
        break;
      case MUXIVA_FRAME_SIGNAL:
        primary_text_ = copy_string(frame.payload.signal.signal_name);
        secondary_text_ = copy_string(frame.payload.signal.source_node_id);
        payload_bytes_ = copy_bytes(frame.payload.signal.value);
        break;
      case MUXIVA_FRAME_EVENT:
        primary_text_ = copy_string(frame.payload.event.topic);
        payload_bytes_ = copy_bytes(frame.payload.event.value);
        break;
      default:
        break;
    }
    rebind();
  }

  void rebind() noexcept {
    frame.header.frame_id = borrow(frame_id_);
    frame.header.clock_domain_id = borrow(clock_domain_id_);
    frame.header.stream_id = borrow(stream_id_);
    frame.header.trace_id = borrow(trace_id_);
    switch (frame.header.frame_type) {
      case MUXIVA_FRAME_AUDIO:
        frame.payload.audio.bytes = borrow(payload_bytes_);
        break;
      case MUXIVA_FRAME_VIDEO:
        frame.payload.video.bytes = borrow(payload_bytes_);
        break;
      case MUXIVA_FRAME_TEXT:
        frame.payload.text.text = borrow(primary_text_);
        frame.payload.text.media_type = borrow(secondary_text_);
        break;
      case MUXIVA_FRAME_BYTE:
        frame.payload.bytes.bytes = borrow(payload_bytes_);
        frame.payload.bytes.media_type = borrow(primary_text_);
        break;
      case MUXIVA_FRAME_SIGNAL:
        frame.payload.signal.signal_name = borrow(primary_text_);
        frame.payload.signal.source_node_id = borrow(secondary_text_);
        frame.payload.signal.value = borrow(payload_bytes_);
        break;
      case MUXIVA_FRAME_EVENT:
        frame.payload.event.topic = borrow(primary_text_);
        frame.payload.event.value = borrow(payload_bytes_);
        break;
      default:
        break;
    }
  }

  std::string frame_id_;
  std::string clock_domain_id_;
  std::string stream_id_;
  std::string trace_id_;
  std::string primary_text_;
  std::string secondary_text_;
  std::vector<std::uint8_t> payload_bytes_;
};

namespace detail {
struct OwnedFrameAccess;
}

/// A move-only Frame whose Audio, Video, or Byte payload can be transferred to
/// the Runtime without copying. Header strings are copied because they are
/// small; the potentially large media allocation is moved.
class OwnedFrame final {
 public:
  OwnedFrame(const muxiva_frame_view_v1& value,
             std::vector<std::uint8_t> payload)
      : frame_(value), payload_(std::make_unique<std::vector<std::uint8_t>>(
                           std::move(payload))) {
    const auto declared = declared_payload_size(frame_);
    if (declared != payload_->size()) {
      throw std::invalid_argument(
          "OwnedFrame payload size must match the Frame view");
    }
    frame_id_ = copy_string(frame_.header.frame_id);
    clock_domain_id_ = copy_string(frame_.header.clock_domain_id);
    stream_id_ = copy_string(frame_.header.stream_id);
    trace_id_ = copy_string(frame_.header.trace_id);
    if (frame_.header.frame_type == MUXIVA_FRAME_BYTE) {
      media_type_ = copy_string(frame_.payload.bytes.media_type);
    }
    rebind();
  }

  OwnedFrame(const OwnedFrame&) = delete;
  OwnedFrame& operator=(const OwnedFrame&) = delete;

  OwnedFrame(OwnedFrame&& other) noexcept
      : frame_(other.frame_), payload_(std::move(other.payload_)),
        frame_id_(std::move(other.frame_id_)),
        clock_domain_id_(std::move(other.clock_domain_id_)),
        stream_id_(std::move(other.stream_id_)),
        trace_id_(std::move(other.trace_id_)),
        media_type_(std::move(other.media_type_)) {
    rebind();
  }

  OwnedFrame& operator=(OwnedFrame&& other) noexcept {
    if (this == &other) return *this;
    frame_ = other.frame_;
    payload_ = std::move(other.payload_);
    frame_id_ = std::move(other.frame_id_);
    clock_domain_id_ = std::move(other.clock_domain_id_);
    stream_id_ = std::move(other.stream_id_);
    trace_id_ = std::move(other.trace_id_);
    media_type_ = std::move(other.media_type_);
    rebind();
    return *this;
  }

  const muxiva_frame_view_v1& view() const noexcept { return frame_; }

 private:
  friend struct detail::OwnedFrameAccess;

  /// Releases the native payload owner to the ABI bridge. Application Nodes
  /// should use GraphNodeContext::emit_owned instead of calling this directly.
  std::vector<std::uint8_t>* release_payload() noexcept {
    return payload_.release();
  }
  static std::string copy_string(muxiva_str_v1 value) {
    return value.data == nullptr ? std::string{}
                                 : std::string(value.data, value.len);
  }

  static muxiva_str_v1 borrow(const std::string& value) noexcept {
    return {value.data(), value.size()};
  }

  static muxiva_bytes_v1 borrow(
      const std::vector<std::uint8_t>& value) noexcept {
    return {value.data(), value.size()};
  }

  static std::size_t declared_payload_size(
      const muxiva_frame_view_v1& frame) {
    switch (frame.header.frame_type) {
      case MUXIVA_FRAME_AUDIO:
        return frame.payload.audio.bytes.len;
      case MUXIVA_FRAME_VIDEO:
        return frame.payload.video.bytes.len;
      case MUXIVA_FRAME_BYTE:
        return frame.payload.bytes.bytes.len;
      default:
        throw std::invalid_argument(
            "OwnedFrame supports Audio, Video, and Byte payloads");
    }
  }

  void rebind() noexcept {
    frame_.header.frame_id = borrow(frame_id_);
    frame_.header.clock_domain_id = borrow(clock_domain_id_);
    frame_.header.stream_id = borrow(stream_id_);
    frame_.header.trace_id = borrow(trace_id_);
    if (!payload_) return;
    switch (frame_.header.frame_type) {
      case MUXIVA_FRAME_AUDIO:
        frame_.payload.audio.bytes = borrow(*payload_);
        break;
      case MUXIVA_FRAME_VIDEO:
        frame_.payload.video.bytes = borrow(*payload_);
        break;
      case MUXIVA_FRAME_BYTE:
        frame_.payload.bytes.bytes = borrow(*payload_);
        frame_.payload.bytes.media_type = borrow(media_type_);
        break;
      default:
        break;
    }
  }

  muxiva_frame_view_v1 frame_{};
  std::unique_ptr<std::vector<std::uint8_t>> payload_;
  std::string frame_id_;
  std::string clock_domain_id_;
  std::string stream_id_;
  std::string trace_id_;
  std::string media_type_;
};

namespace detail {
struct OwnedFrameAccess {
  static std::vector<std::uint8_t>* release(OwnedFrame& frame) noexcept {
    return frame.release_payload();
  }
};
}  // namespace detail

struct OwnedGraphEmission {
  std::string output_port;
  OwnedFrame frame;

  OwnedGraphEmission(std::string port, OwnedFrame value)
      : output_port(std::move(port)), frame(std::move(value)) {}

  OwnedGraphEmission(const OwnedGraphEmission&) = delete;
  OwnedGraphEmission& operator=(const OwnedGraphEmission&) = delete;
  OwnedGraphEmission(OwnedGraphEmission&&) noexcept = default;
  OwnedGraphEmission& operator=(OwnedGraphEmission&&) noexcept = default;
};

struct GraphMetric {
  std::string name;
  std::uint32_t operation = MUXIVA_NODE_METRIC_GAUGE_SET;
  std::uint64_t value = 0;
};

class GraphNodeContext {
 public:
  explicit GraphNodeContext(std::string_view input_port,
                            bool supports_owned_emissions = false)
      : input_port_(input_port),
        supports_owned_emissions_(supports_owned_emissions) {}
  std::string_view input_port() const noexcept { return input_port_; }
  void emit(std::string output_port, muxiva_frame_view_v1 frame) {
    emissions_.push_back({std::move(output_port), frame});
  }
  void emit(GraphEmission emission) {
    emissions_.push_back(std::move(emission));
  }
  /// Transfers a large immutable media payload to the Runtime when supported.
  /// Older hosts transparently receive the same Frame through the safe-copy
  /// path, so Node Packs remain backward compatible.
  void emit_owned(std::string output_port, OwnedFrame frame) {
    if (supports_owned_emissions_) {
      owned_emissions_.emplace_back(std::move(output_port), std::move(frame));
    } else {
      emit(std::move(output_port), frame.view());
    }
  }
  void schedule_next_tick(std::chrono::nanoseconds delay) {
    next_source_tick_ns_ = delay.count() > 0
        ? static_cast<std::uint64_t>(delay.count()) : 1;
  }
  void increment_counter(std::string name, std::uint64_t delta = 1) {
    metrics_.push_back({std::move(name), MUXIVA_NODE_METRIC_COUNTER_ADD, delta});
  }
  void set_gauge(std::string name, std::uint64_t value) {
    metrics_.push_back({std::move(name), MUXIVA_NODE_METRIC_GAUGE_SET, value});
  }
  std::vector<GraphEmission> take_emissions() { return std::move(emissions_); }
  std::vector<OwnedGraphEmission> take_owned_emissions() {
    return std::move(owned_emissions_);
  }
  std::vector<GraphMetric> take_metrics() { return std::move(metrics_); }
  std::uint64_t take_next_source_tick_ns() noexcept {
    const auto value = next_source_tick_ns_;
    next_source_tick_ns_ = 0;
    return value;
  }

 private:
  std::string_view input_port_;
  std::vector<GraphEmission> emissions_;
  std::vector<OwnedGraphEmission> owned_emissions_;
  std::vector<GraphMetric> metrics_;
  std::uint64_t next_source_tick_ns_ = 0;
  bool supports_owned_emissions_ = false;
};

class MultimodalGraphNode {
 public:
  virtual ~MultimodalGraphNode() = default;
  virtual void on_prepare() {}
  virtual void on_process(const muxiva_frame_view_v1* input,
                          GraphNodeContext& context) {
    for (auto& emission : on_process(input, context.input_port())) {
      context.emit(std::move(emission));
    }
  }
  // V1 source compatibility. New Nodes should override the context form.
  virtual std::vector<GraphEmission> on_process(
      const muxiva_frame_view_v1*, std::string_view) { return {}; }
  virtual void on_signal(const muxiva_frame_view_v1&) {}
  virtual void on_finish() {}
  virtual void on_abort(const muxiva_abort_reason_v1&) noexcept {}
};

namespace detail {
struct MultimodalNodeBox {
  explicit MultimodalNodeBox(MultimodalGraphNode* value,
                             bool supports_owned)
      : implementation(value), supports_owned_emissions(supports_owned) {}
  std::unique_ptr<MultimodalGraphNode> implementation;
  std::vector<GraphEmission> emissions;
  std::vector<OwnedGraphEmission> owned_emissions;
  std::vector<muxiva_named_frame_v1> views;
  std::vector<muxiva_owned_named_frame_v1> owned_views;
  std::vector<GraphMetric> metrics;
  std::vector<muxiva_node_metric_v1> metric_views;
  std::uint64_t next_source_tick_ns = 0;
  bool supports_owned_emissions = false;
  bool owned_emissions_taken = true;
};

inline void release_owned_payload(void* value) noexcept {
  try {
    delete static_cast<std::vector<std::uint8_t>*>(value);
  } catch (...) {
  }
}
inline muxiva_status_v1 multimodal_prepare(void* data, muxiva_error_v1* error) noexcept {
  try { static_cast<MultimodalNodeBox*>(data)->implementation->on_prepare(); return MUXIVA_STATUS_OK; }
  catch (...) { write_exception(error); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}
inline muxiva_status_v1 multimodal_process(
    void* data, const muxiva_frame_view_v1* input, muxiva_str_v1 input_port,
    const muxiva_named_frame_v1** output, size_t* output_count,
    muxiva_error_v1* error) noexcept {
  try {
    if (data == nullptr || output == nullptr || output_count == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
    auto* box = static_cast<MultimodalNodeBox*>(data);
    const std::string_view port(input_port.data == nullptr ? "" : input_port.data,
                                input_port.data == nullptr ? 0 : input_port.len);
    GraphNodeContext context(port, box->supports_owned_emissions);
    box->implementation->on_process(input, context);
    box->emissions = context.take_emissions();
    box->owned_emissions = context.take_owned_emissions();
    box->owned_views.clear();
    box->owned_emissions_taken = false;
    box->metrics = context.take_metrics();
    box->next_source_tick_ns = context.take_next_source_tick_ns();
    box->views.clear();
    box->views.reserve(box->emissions.size());
    for (const auto& emission : box->emissions) {
      box->views.push_back({{emission.output_port.data(), emission.output_port.size()}, emission.frame});
    }
    *output = box->views.empty() ? nullptr : box->views.data();
    *output_count = box->views.size();
    return MUXIVA_STATUS_OK;
  } catch (...) { write_exception(error); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}
inline void multimodal_take_owned_emissions(
    void* data, const muxiva_owned_named_frame_v1** output,
    size_t* output_count) noexcept {
  if (data == nullptr || output == nullptr || output_count == nullptr) return;
  auto* box = static_cast<MultimodalNodeBox*>(data);
  if (box->owned_emissions_taken) {
    *output = nullptr;
    *output_count = 0;
    return;
  }
  box->owned_views.clear();
  box->owned_views.reserve(box->owned_emissions.size());
  for (auto& emission : box->owned_emissions) {
    box->owned_views.push_back(
        {{emission.output_port.data(), emission.output_port.size()},
         emission.frame.view(), OwnedFrameAccess::release(emission.frame),
         release_owned_payload, {0, 0}});
  }
  box->owned_emissions_taken = true;
  *output = box->owned_views.empty() ? nullptr : box->owned_views.data();
  *output_count = box->owned_views.size();
}
inline void multimodal_take_metrics(
    void* data, const muxiva_node_metric_v1** output,
    size_t* output_count) noexcept {
  if (data == nullptr || output == nullptr || output_count == nullptr) return;
  auto* box = static_cast<MultimodalNodeBox*>(data);
  box->metric_views.clear();
  box->metric_views.reserve(box->metrics.size());
  for (const auto& metric : box->metrics) {
    box->metric_views.push_back({{metric.name.data(), metric.name.size()},
                                 metric.operation, 0, metric.value});
  }
  *output = box->metric_views.empty() ? nullptr : box->metric_views.data();
  *output_count = box->metric_views.size();
  // Keep the owning strings alive until the next lifecycle call. The Rust host
  // copies these borrowed views immediately after this callback returns.
}
inline muxiva_status_v1 multimodal_finish(void* data, muxiva_error_v1* error) noexcept {
  try { static_cast<MultimodalNodeBox*>(data)->implementation->on_finish(); return MUXIVA_STATUS_OK; }
  catch (...) { write_exception(error); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}
inline muxiva_status_v1 multimodal_signal(
    void* data, const muxiva_frame_view_v1* signal, muxiva_error_v1* error) noexcept {
  try {
    if (data == nullptr || signal == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
    static_cast<MultimodalNodeBox*>(data)->implementation->on_signal(*signal);
    return MUXIVA_STATUS_OK;
  } catch (...) { write_exception(error); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}
inline void multimodal_abort(void* data, const muxiva_abort_reason_v1* reason) noexcept {
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
inline muxiva_graph_node_vtable_v1 multimodal_vtable(
    MultimodalGraphNode* implementation, bool supports_owned_emissions) {
  muxiva_graph_node_vtable_v1 table{};
  table.abi_version = MUXIVA_ABI_VERSION_V1;
  table.struct_size = sizeof(table);
  table.user_data =
      new MultimodalNodeBox(implementation, supports_owned_emissions);
  table.on_prepare = multimodal_prepare;
  table.on_process = multimodal_process;
  table.on_signal = multimodal_signal;
  table.on_finish = multimodal_finish;
  table.on_abort = multimodal_abort;
  table.destroy = multimodal_destroy;
  table.take_next_source_tick_ns = multimodal_take_next_source_tick;
  table.take_metrics = multimodal_take_metrics;
  table.take_owned_emissions = supports_owned_emissions
                                   ? multimodal_take_owned_emissions
                                   : nullptr;
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
                  "T must derive from muxiva::MultimodalGraphNode");
    return MultimodalGraphNodeFactory(
        std::move(node_type), kind, std::move(ports_json),
        [](const std::string& config) -> MultimodalGraphNode* {
          if constexpr (std::is_constructible<T, const std::string&>::value) return new T(config);
          else return new T();
        }, std::move(config_schema_json), std::move(version));
  }

  muxiva_multimodal_node_factory_v1 view() const noexcept {
    muxiva_multimodal_node_factory_v1 result{};
    result.abi_version = MUXIVA_ABI_VERSION_V1; result.struct_size = sizeof(result);
    result.node_type = borrow(node_type_); result.version = borrow(version_); result.kind = kind_;
    result.ports_json = borrow(ports_json_); result.config_schema_json = borrow(config_schema_json_);
    result.user_data = creator_.get(); result.create = create;
    return result;
  }

 private:
  static muxiva_str_v1 borrow(const std::string& value) noexcept { return {value.data(), value.size()}; }
  static muxiva_status_v1 create(void* data, muxiva_str_v1, muxiva_str_v1 config,
                               muxiva_graph_node_vtable_v1* output,
                               muxiva_error_v1* error) noexcept {
    try {
      if (data == nullptr || output == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
      const std::string config_value(config.data == nullptr ? "" : config.data,
                                     config.data == nullptr ? 0 : config.len);
      auto* implementation = (*static_cast<Creator*>(data))(config_value);
      if (implementation == nullptr) return MUXIVA_STATUS_INVALID_ARGUMENT;
      const auto caller_size = static_cast<std::size_t>(output->struct_size);
      constexpr auto legacy_size = offsetof(muxiva_graph_node_vtable_v1,
                                             take_next_source_tick_ns);
      if (caller_size < legacy_size) {
        delete implementation;
        return MUXIVA_STATUS_INVALID_ARGUMENT;
      }
      const bool supports_owned_emissions =
          caller_size >= sizeof(muxiva_graph_node_vtable_v1);
      auto table = detail::multimodal_vtable(implementation,
                                            supports_owned_emissions);
      const auto copied_size = std::min(caller_size, sizeof(table));
      std::memcpy(output, &table, copied_size);
      output->struct_size = static_cast<std::uint32_t>(copied_size);
      return MUXIVA_STATUS_OK;
    } catch (...) { detail::write_exception(error); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
  }
  std::string node_type_, version_;
  uint32_t kind_;
  std::string ports_json_, config_schema_json_;
  std::shared_ptr<Creator> creator_;
};

class Runtime final {
 public:
  explicit Runtime(Error& error) {
    const auto status = muxiva_runtime_create_v1(&handle_, error.out());
    if (status != MUXIVA_STATUS_OK) throw std::runtime_error(error.message());
    open_ = true;
  }
  Runtime(const Runtime&) = delete;
  Runtime& operator=(const Runtime&) = delete;
  Runtime(Runtime&& other) noexcept : handle_(other.handle_), open_(other.open_) {
    other.open_ = false;
  }
  ~Runtime() noexcept { close(); }
  muxiva_status_v1 close() noexcept {
    if (!open_) return MUXIVA_STATUS_CLOSED;
    const auto status = muxiva_runtime_release_v1(handle_);
    if (status == MUXIVA_STATUS_OK || status == MUXIVA_STATUS_CLOSED) open_ = false;
    return status;
  }
  muxiva_status_v1 run_text(const Node& node, const muxiva_frame_view_v1& input,
                          std::string& output, Error& error) const {
    char bytes[4096]{};
    size_t length = 0;
    const auto status = muxiva_runtime_run_text_v1(
        handle_, node.get(), &input, bytes, sizeof(bytes), &length, error.out());
    if (status == MUXIVA_STATUS_OK) output.assign(bytes, length);
    return status;
  }
  muxiva_status_v1 run_text(const Node& node, const TextFrame& input,
                          std::string& output, Error& error) const {
    const auto borrowed = input.view();
    return run_text(node, borrowed, output, error);
  }
  muxiva_status_v1 run_graph(const std::string& graph_json,
                           const std::vector<GraphNodeFactory>& factories,
                           uint32_t& worker_total, Error& error,
                           uint64_t timeout_ms = 30000) const {
    std::vector<muxiva_node_factory_v1> views;
    views.reserve(factories.size());
    for (const auto& factory : factories) views.push_back(factory.view());
    muxiva_graph_run_summary_v1 summary{};
    summary.abi_version = MUXIVA_ABI_VERSION_V1;
    summary.struct_size = sizeof(summary);
    const muxiva_str_v1 json{graph_json.data(), graph_json.size()};
    const auto status = muxiva_runtime_run_graph_v1(
        handle_, json, views.data(), views.size(), timeout_ms, &summary,
        error.out());
    if (status == MUXIVA_STATUS_OK) worker_total = summary.worker_total;
    return status;
  }
  muxiva_status_v1 run_multimodal_graph(
      const std::string& graph_json,
      const std::vector<MultimodalGraphNodeFactory>& factories,
      uint32_t& worker_total, Error& error,
      uint64_t timeout_ms = 30000) const {
    std::vector<muxiva_multimodal_node_factory_v1> views;
    views.reserve(factories.size());
    for (const auto& factory : factories) views.push_back(factory.view());
    muxiva_graph_run_summary_v1 summary{};
    summary.abi_version = MUXIVA_ABI_VERSION_V1; summary.struct_size = sizeof(summary);
    const muxiva_str_v1 json{graph_json.data(), graph_json.size()};
    const auto status = muxiva_runtime_run_multimodal_graph_v1(
        handle_, json, views.data(), views.size(), timeout_ms, &summary, error.out());
    if (status == MUXIVA_STATUS_OK) worker_total = summary.worker_total;
    return status;
  }

 private:
  muxiva_runtime_v1 handle_{};
  bool open_ = false;
};

}  // namespace muxiva
#endif
