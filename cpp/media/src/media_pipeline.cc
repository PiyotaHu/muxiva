#include "muxiva/media.hpp"

#include <atomic>
#include <limits>
#include <utility>

namespace muxiva::media {
namespace {

constexpr int kInvalidArgument = -2001;
constexpr int kBusy = -2002;
constexpr int kOversized = -2003;
constexpr int kBackendFailure = -2004;

std::size_t sample_width(AudioSampleFormat format) noexcept {
  switch (format) {
  case AudioSampleFormat::u8:
    return 1;
  case AudioSampleFormat::i16le:
    return 2;
  case AudioSampleFormat::i24le:
    return 3;
  case AudioSampleFormat::i32le:
  case AudioSampleFormat::f32le:
    return 4;
  case AudioSampleFormat::f64le:
    return 8;
  }
  return 0;
}

bool checked_product(std::size_t left, std::size_t right,
                     std::size_t *output) noexcept {
  if (left != 0 && right > std::numeric_limits<std::size_t>::max() / left) {
    return false;
  }
  *output = left * right;
  return true;
}

bool valid_audio_spec(const AudioSpec &spec) noexcept {
  return spec.sample_rate_hz >= 1 && spec.sample_rate_hz <= 768000 &&
         spec.channels >= 1 && spec.channels <= 64 &&
         sample_width(spec.sample_format) != 0;
}

bool audio_size(const AudioSpec &spec, std::uint64_t samples,
                std::size_t *output) noexcept {
  if (samples == 0 || samples > std::numeric_limits<std::size_t>::max())
    return false;
  std::size_t count = 0;
  return checked_product(static_cast<std::size_t>(samples), spec.channels,
                         &count) &&
         checked_product(count, sample_width(spec.sample_format), output);
}

bool video_size(const VideoSpec &spec, std::size_t *output) noexcept {
  if (spec.width == 0 || spec.height == 0)
    return false;
  std::size_t pixels = 0;
  if (!checked_product(spec.width, spec.height, &pixels))
    return false;
  if (spec.pixel_format == PixelFormat::rgba8) {
    return checked_product(pixels, std::size_t{4}, output);
  }
  if (spec.width % 2 != 0 || spec.height % 2 != 0)
    return false;
  if (pixels > std::numeric_limits<std::size_t>::max() - pixels / 2)
    return false;
  *output = pixels + pixels / 2;
  return true;
}

bool duration_ns(std::uint64_t samples, std::uint32_t rate,
                 std::int64_t *output) noexcept {
  if (rate == 0 || samples > static_cast<std::uint64_t>(
                                 std::numeric_limits<std::int64_t>::max())) {
    return false;
  }
  const auto whole = samples / rate;
  const auto remainder = samples % rate;
  if (whole >
      static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max()) /
          1000000000ULL) {
    return false;
  }
  const auto value = whole * 1000000000ULL + remainder * 1000000000ULL / rate;
  if (value >
      static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max()))
    return false;
  *output = static_cast<std::int64_t>(value);
  return true;
}

std::uint64_t distance(std::int64_t left, std::int64_t right) noexcept {
  if (left >= right)
    return static_cast<std::uint64_t>(left) - static_cast<std::uint64_t>(right);
  return static_cast<std::uint64_t>(right) - static_cast<std::uint64_t>(left);
}

} // namespace

Status Status::failure(int code, const char *message) noexcept {
  try {
    return {code, message == nullptr ? std::string{} : std::string{message}};
  } catch (...) {
    return {code, {}};
  }
}

struct Pipeline::Impl final {
  Impl(PipelineConfig value, std::unique_ptr<Backend> implementation) noexcept
      : config(value), backend(std::move(implementation)) {}

  struct Admission final {
    explicit Admission(Impl &value) noexcept : impl(value) {
      admitted = !impl.busy.test_and_set(std::memory_order_acquire);
      if (!admitted)
        impl.busy_rejected.fetch_add(1, std::memory_order_relaxed);
    }
    ~Admission() {
      if (admitted)
        impl.busy.clear(std::memory_order_release);
    }
    Impl &impl;
    bool admitted = false;
  };

  PipelineConfig config;
  std::unique_ptr<Backend> backend;
  std::atomic_flag busy = ATOMIC_FLAG_INIT;
  std::atomic<std::uint64_t> audio_frames{0};
  std::atomic<std::uint64_t> video_frames{0};
  std::atomic<std::uint64_t> output_bytes{0};
  std::atomic<std::uint64_t> invalid_rejected{0};
  std::atomic<std::uint64_t> oversized_rejected{0};
  std::atomic<std::uint64_t> busy_rejected{0};
  std::atomic<std::uint64_t> backend_failures{0};
  std::atomic<std::uint64_t> timestamp_discontinuities{0};
  bool has_audio_timestamp = false;
  std::int64_t next_audio_timestamp = 0;
  bool has_audio_spec = false;
  bool audio_flushing = false;
  AudioSpec audio_input_spec{};
  AudioSpec audio_output_spec{};
};

std::unique_ptr<Pipeline> Pipeline::create(PipelineConfig config,
                                           std::unique_ptr<Backend> backend,
                                           Status *status) noexcept {
  auto fail = [&](int code, const char *message) {
    if (status != nullptr)
      *status = Status::failure(code, message);
    return std::unique_ptr<Pipeline>{};
  };
  if (!backend || config.max_frame_bytes == 0 ||
      config.max_frame_bytes > 512U * 1024U * 1024U ||
      config.audio_timestamp_tolerance.count() < 0) {
    return fail(kInvalidArgument, "invalid media pipeline configuration");
  }
  try {
    auto pipeline = std::unique_ptr<Pipeline>(
        new Pipeline(std::make_unique<Impl>(config, std::move(backend))));
    if (status != nullptr)
      *status = Status::success();
    return pipeline;
  } catch (...) {
    return fail(kInvalidArgument, "failed to allocate media pipeline");
  }
}

Pipeline::Pipeline(std::unique_ptr<Impl> impl) noexcept
    : impl_(std::move(impl)) {}
Pipeline::~Pipeline() noexcept = default;

Status Pipeline::convert_audio(const PackedAudioView &input,
                               const AudioSpec &output_spec,
                               OwnedAudioFrame *output) noexcept {
  if (output == nullptr)
    return Status::failure(kInvalidArgument, "audio output is null");
  Impl::Admission admission(*impl_);
  if (!admission.admitted)
    return Status::failure(kBusy, "media pipeline is busy");
  std::size_t input_size = 0;
  if (!valid_audio_spec(input.spec) || !valid_audio_spec(output_spec) ||
      !audio_size(input.spec, input.samples_per_channel, &input_size) ||
      input_size != input.size || input.data == nullptr) {
    impl_->invalid_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kInvalidArgument, "invalid packed audio frame");
  }
  if (input.size > impl_->config.max_frame_bytes) {
    impl_->oversized_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kOversized, "audio input exceeds media budget");
  }
  if (impl_->audio_flushing ||
      (impl_->has_audio_spec && (!(impl_->audio_input_spec == input.spec) ||
                                 !(impl_->audio_output_spec == output_spec)))) {
    impl_->invalid_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(
        kInvalidArgument,
        "audio stream format changed or was flushed; reset is required");
  }
  OwnedAudioFrame converted;
  const int result = impl_->backend->convert_audio(
      input, output_spec, impl_->config.max_frame_bytes, &converted);
  if (result != 0) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(result, "audio backend conversion failed");
  }
  impl_->has_audio_spec = true;
  impl_->audio_input_spec = input.spec;
  impl_->audio_output_spec = output_spec;
  if (converted.empty()) {
    converted.bytes.clear();
    converted.spec = output_spec;
    converted.timestamp_ns = impl_->has_audio_timestamp
                                 ? impl_->next_audio_timestamp
                                 : input.timestamp_ns;
    *output = std::move(converted);
    return Status::success();
  }
  std::size_t expected = 0;
  if (!(converted.spec == output_spec) ||
      !audio_size(converted.spec, converted.samples_per_channel, &expected) ||
      expected != converted.bytes.size()) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kBackendFailure,
                           "audio backend returned an invalid frame");
  }
  if (expected > impl_->config.max_frame_bytes) {
    impl_->oversized_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kOversized, "audio output exceeds media budget");
  }
  std::int64_t timestamp = input.timestamp_ns;
  if (impl_->has_audio_timestamp) {
    const auto tolerance = static_cast<std::uint64_t>(
        impl_->config.audio_timestamp_tolerance.count());
    if (distance(input.timestamp_ns, impl_->next_audio_timestamp) <=
        tolerance) {
      timestamp = impl_->next_audio_timestamp;
    } else {
      impl_->timestamp_discontinuities.fetch_add(1, std::memory_order_relaxed);
    }
  }
  std::int64_t duration = 0;
  if (!duration_ns(converted.samples_per_channel, output_spec.sample_rate_hz,
                   &duration) ||
      (duration > 0 &&
       timestamp > std::numeric_limits<std::int64_t>::max() - duration)) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kBackendFailure,
                           "audio timestamp arithmetic overflowed");
  }
  converted.timestamp_ns = timestamp;
  impl_->next_audio_timestamp = timestamp + duration;
  impl_->has_audio_timestamp = true;
  impl_->audio_frames.fetch_add(1, std::memory_order_relaxed);
  impl_->output_bytes.fetch_add(expected, std::memory_order_relaxed);
  *output = std::move(converted);
  return Status::success();
}

Status Pipeline::flush_audio(const AudioSpec &output_spec,
                             OwnedAudioFrame *output) noexcept {
  if (output == nullptr || !valid_audio_spec(output_spec)) {
    return Status::failure(kInvalidArgument, "invalid audio flush request");
  }
  Impl::Admission admission(*impl_);
  if (!admission.admitted)
    return Status::failure(kBusy, "media pipeline is busy");
  if (impl_->has_audio_spec && !(impl_->audio_output_spec == output_spec)) {
    impl_->invalid_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kInvalidArgument,
                           "audio flush format does not match the stream");
  }
  impl_->audio_flushing = impl_->has_audio_spec;
  OwnedAudioFrame converted;
  const int result = impl_->backend->flush_audio(
      output_spec, impl_->config.max_frame_bytes, &converted);
  if (result != 0) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(result, "audio backend flush failed");
  }
  converted.spec = output_spec;
  converted.timestamp_ns =
      impl_->has_audio_timestamp ? impl_->next_audio_timestamp : 0;
  if (converted.empty()) {
    converted.bytes.clear();
    *output = std::move(converted);
    return Status::success();
  }
  std::size_t expected = 0;
  if (!audio_size(output_spec, converted.samples_per_channel, &expected) ||
      expected != converted.bytes.size() ||
      expected > impl_->config.max_frame_bytes) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kBackendFailure,
                           "audio backend returned an invalid flush frame");
  }
  std::int64_t duration = 0;
  if (!duration_ns(converted.samples_per_channel, output_spec.sample_rate_hz,
                   &duration) ||
      (duration > 0 &&
       converted.timestamp_ns >
           std::numeric_limits<std::int64_t>::max() - duration)) {
    return Status::failure(kBackendFailure, "audio flush timestamp overflowed");
  }
  impl_->next_audio_timestamp = converted.timestamp_ns + duration;
  impl_->has_audio_timestamp = true;
  impl_->audio_frames.fetch_add(1, std::memory_order_relaxed);
  impl_->output_bytes.fetch_add(expected, std::memory_order_relaxed);
  *output = std::move(converted);
  return Status::success();
}

Status Pipeline::convert_video(const PackedVideoView &input,
                               const VideoSpec &output_spec,
                               OwnedVideoFrame *output) noexcept {
  if (output == nullptr)
    return Status::failure(kInvalidArgument, "video output is null");
  Impl::Admission admission(*impl_);
  if (!admission.admitted)
    return Status::failure(kBusy, "media pipeline is busy");
  std::size_t input_size = 0;
  std::size_t output_size = 0;
  if (!video_size(input.spec, &input_size) || input_size != input.size ||
      input.data == nullptr || !video_size(output_spec, &output_size)) {
    impl_->invalid_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kInvalidArgument, "invalid packed video frame");
  }
  if (input_size > impl_->config.max_frame_bytes ||
      output_size > impl_->config.max_frame_bytes) {
    impl_->oversized_rejected.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kOversized, "video frame exceeds media budget");
  }
  OwnedVideoFrame converted;
  const int result = impl_->backend->convert_video(
      input, output_spec, impl_->config.max_frame_bytes, &converted);
  if (result != 0) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(result, "video backend conversion failed");
  }
  if (!(converted.spec == output_spec) ||
      converted.bytes.size() != output_size) {
    impl_->backend_failures.fetch_add(1, std::memory_order_relaxed);
    return Status::failure(kBackendFailure,
                           "video backend returned an invalid frame");
  }
  converted.timestamp_ns = input.timestamp_ns;
  impl_->video_frames.fetch_add(1, std::memory_order_relaxed);
  impl_->output_bytes.fetch_add(output_size, std::memory_order_relaxed);
  *output = std::move(converted);
  return Status::success();
}

Status Pipeline::reset() noexcept {
  Impl::Admission admission(*impl_);
  if (!admission.admitted)
    return Status::failure(kBusy, "media pipeline is busy");
  impl_->backend->reset();
  impl_->has_audio_timestamp = false;
  impl_->next_audio_timestamp = 0;
  impl_->has_audio_spec = false;
  impl_->audio_flushing = false;
  return Status::success();
}

const char *Pipeline::backend_name() const noexcept {
  return impl_->backend->name();
}

PipelineStats Pipeline::stats() const noexcept {
  return {impl_->audio_frames.load(std::memory_order_relaxed),
          impl_->video_frames.load(std::memory_order_relaxed),
          impl_->output_bytes.load(std::memory_order_relaxed),
          impl_->invalid_rejected.load(std::memory_order_relaxed),
          impl_->oversized_rejected.load(std::memory_order_relaxed),
          impl_->busy_rejected.load(std::memory_order_relaxed),
          impl_->backend_failures.load(std::memory_order_relaxed),
          impl_->timestamp_discontinuities.load(std::memory_order_relaxed)};
}

} // namespace muxiva::media

#if !defined(MUXIVA_ENABLE_FFMPEG)
namespace muxiva::media {
std::unique_ptr<Backend> make_ffmpeg_backend() noexcept { return {}; }
} // namespace muxiva::media
#endif
