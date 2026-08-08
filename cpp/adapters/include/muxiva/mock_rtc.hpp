#ifndef MUXIVA_MOCK_RTC_HPP
#define MUXIVA_MOCK_RTC_HPP
#include "muxiva/rtc_adapter_v1.h"
#include <utility>

namespace muxiva {
class MockRtc final {
 public:
  MockRtc() noexcept = default;
  explicit MockRtc(muxiva_rtc_adapter_handle_v1* value) noexcept : value_(value) {}
  MockRtc(const MockRtc&) = delete;
  MockRtc& operator=(const MockRtc&) = delete;
  MockRtc(MockRtc&& other) noexcept : value_(std::exchange(other.value_, nullptr)) {}
  MockRtc& operator=(MockRtc&& other) noexcept { if (this != &other) { reset(); value_ = std::exchange(other.value_, nullptr); } return *this; }
  ~MockRtc() noexcept { reset(); }
  void reset() noexcept { if (value_) muxiva_rtc_adapter_destroy_v1(std::exchange(value_, nullptr)); }
  muxiva_rtc_adapter_handle_v1* get() const noexcept { return value_; }
 private:
  muxiva_rtc_adapter_handle_v1* value_ = nullptr;
};
}
#endif
