#ifndef VOXA_VOXA_HPP
#define VOXA_VOXA_HPP

#include "voxa.h"

#include <algorithm>
#include <cstring>
#include <stdexcept>
#include <string>
#include <utility>

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
}  // namespace detail

class Node final {
 public:
  Node() noexcept = default;
  explicit Node(TransformNode* implementation, Error& error) {
    voxa_node_vtable_v1 table{};
    table.abi_version = VOXA_ABI_VERSION_V1;
    table.struct_size = sizeof(table);
    table.user_data = implementation;
    table.on_prepare = detail::prepare;
    table.on_process = detail::process;
    table.on_signal = detail::signal;
    table.on_finish = detail::finish;
    table.on_abort = detail::abort;
    table.destroy = detail::destroy;
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

 private:
  voxa_runtime_v1 handle_{};
  bool open_ = false;
};

}  // namespace voxa
#endif
