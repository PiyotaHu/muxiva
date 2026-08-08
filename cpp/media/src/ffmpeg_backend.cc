#include "muxiva/media.hpp"

extern "C" {
#include <libavutil/channel_layout.h>
#include <libavutil/error.h>
#include <libavutil/samplefmt.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
}

#include <algorithm>
#include <cerrno>
#include <cstdint>
#include <iterator>
#include <limits>
#include <memory>

namespace muxiva::media {
namespace {

constexpr int kInvalid = AVERROR(EINVAL);

AVSampleFormat audio_format(AudioSampleFormat format) noexcept {
  switch (format) {
  case AudioSampleFormat::u8:
    return AV_SAMPLE_FMT_U8;
  case AudioSampleFormat::i16le:
    return AV_SAMPLE_FMT_S16;
  case AudioSampleFormat::i24le:
    return AV_SAMPLE_FMT_NONE;
  case AudioSampleFormat::i32le:
    return AV_SAMPLE_FMT_S32;
  case AudioSampleFormat::f32le:
    return AV_SAMPLE_FMT_FLT;
  case AudioSampleFormat::f64le:
    return AV_SAMPLE_FMT_DBL;
  }
  return AV_SAMPLE_FMT_NONE;
}

AVPixelFormat pixel_format(PixelFormat format) noexcept {
  return format == PixelFormat::rgba8 ? AV_PIX_FMT_RGBA : AV_PIX_FMT_YUV420P;
}

bool fits_int(std::uint64_t value) noexcept {
  return value <= static_cast<std::uint64_t>(std::numeric_limits<int>::max());
}

class FfmpegBackend final : public Backend {
public:
  ~FfmpegBackend() override { reset(); }

  const char *name() const noexcept override { return "ffmpeg"; }

  int convert_audio(const PackedAudioView &input, const AudioSpec &output_spec,
                    std::size_t max_output_bytes,
                    OwnedAudioFrame *output) noexcept override {
    try {
      if (output == nullptr || !fits_int(input.samples_per_channel))
        return kInvalid;
      int result = configure_audio(input.spec, output_spec);
      if (result < 0)
        return result;
      const int capacity = swr_get_out_samples(
          resampler_, static_cast<int>(input.samples_per_channel));
      if (capacity < 0)
        return capacity;
      result = allocate_audio(output_spec, capacity, max_output_bytes, output);
      if (result < 0)
        return result;
      std::uint8_t *output_planes[] = {output->bytes.data()};
      const std::uint8_t *input_planes[] = {input.data};
      const int samples =
          swr_convert(resampler_, output_planes, capacity, input_planes,
                      static_cast<int>(input.samples_per_channel));
      if (samples < 0)
        return samples;
      return finish_audio(output_spec, samples, output);
    } catch (...) {
      return AVERROR(ENOMEM);
    }
  }

  int flush_audio(const AudioSpec &output_spec, std::size_t max_output_bytes,
                  OwnedAudioFrame *output) noexcept override {
    try {
      if (output == nullptr)
        return kInvalid;
      output->bytes.clear();
      output->spec = output_spec;
      output->samples_per_channel = 0;
      if (resampler_ == nullptr || !(output_spec == audio_output_) ||
          audio_drained_) {
        return 0;
      }
      const int capacity = swr_get_out_samples(resampler_, 0);
      if (capacity <= 0)
        return capacity;
      int result =
          allocate_audio(output_spec, capacity, max_output_bytes, output);
      if (result < 0)
        return result;
      std::uint8_t *output_planes[] = {output->bytes.data()};
      const int samples =
          swr_convert(resampler_, output_planes, capacity, nullptr, 0);
      if (samples < 0)
        return samples;
      audio_drained_ = true;
      return finish_audio(output_spec, samples, output);
    } catch (...) {
      return AVERROR(ENOMEM);
    }
  }

  int convert_video(const PackedVideoView &input, const VideoSpec &output_spec,
                    std::size_t max_output_bytes,
                    OwnedVideoFrame *output) noexcept override {
    try {
      if (output == nullptr ||
          input.spec.width > std::numeric_limits<int>::max() ||
          input.spec.height > std::numeric_limits<int>::max() ||
          output_spec.width > std::numeric_limits<int>::max() ||
          output_spec.height > std::numeric_limits<int>::max()) {
        return kInvalid;
      }
      const auto input_format = pixel_format(input.spec.pixel_format);
      const auto output_format = pixel_format(output_spec.pixel_format);
      scaler_ = sws_getCachedContext(
          scaler_, static_cast<int>(input.spec.width),
          static_cast<int>(input.spec.height), input_format,
          static_cast<int>(output_spec.width),
          static_cast<int>(output_spec.height), output_format, SWS_BILINEAR,
          nullptr, nullptr, nullptr);
      if (scaler_ == nullptr)
        return AVERROR(ENOMEM);

      const std::size_t output_size = packed_video_size(output_spec);
      if (output_size == 0 || output_size > max_output_bytes)
        return AVERROR(ENOSPC);
      output->bytes.assign(output_size, 0);
      output->spec = output_spec;
      output->timestamp_ns = input.timestamp_ns;

      const std::uint8_t *source[4]{};
      int source_stride[4]{};
      video_planes(input.data, input.spec, source, source_stride);
      std::uint8_t *destination[4]{};
      int destination_stride[4]{};
      video_planes(output->bytes.data(), output_spec, destination,
                   destination_stride);
      const int rows = sws_scale(scaler_, source, source_stride, 0,
                                 static_cast<int>(input.spec.height),
                                 destination, destination_stride);
      return rows == static_cast<int>(output_spec.height) ? 0 : AVERROR(EIO);
    } catch (...) {
      return AVERROR(ENOMEM);
    }
  }

  void reset() noexcept override {
    swr_free(&resampler_);
    if (scaler_ != nullptr)
      sws_freeContext(scaler_);
    scaler_ = nullptr;
    has_audio_spec_ = false;
    audio_drained_ = false;
  }

private:
  int configure_audio(const AudioSpec &input,
                      const AudioSpec &output) noexcept {
    const auto input_format = audio_format(input.sample_format);
    const auto output_format = audio_format(output.sample_format);
    if (input_format == AV_SAMPLE_FMT_NONE ||
        output_format == AV_SAMPLE_FMT_NONE) {
      return AVERROR(ENOSYS);
    }
    if (has_audio_spec_ && input == audio_input_ && output == audio_output_)
      return 0;
    swr_free(&resampler_);
    has_audio_spec_ = false;
    AVChannelLayout input_layout{};
    AVChannelLayout output_layout{};
    av_channel_layout_default(&input_layout, input.channels);
    av_channel_layout_default(&output_layout, output.channels);
    const int allocated = swr_alloc_set_opts2(
        &resampler_, &output_layout, output_format,
        static_cast<int>(output.sample_rate_hz), &input_layout, input_format,
        static_cast<int>(input.sample_rate_hz), 0, nullptr);
    av_channel_layout_uninit(&input_layout);
    av_channel_layout_uninit(&output_layout);
    if (allocated < 0)
      return allocated;
    const int initialized = swr_init(resampler_);
    if (initialized < 0) {
      swr_free(&resampler_);
      return initialized;
    }
    audio_input_ = input;
    audio_output_ = output;
    has_audio_spec_ = true;
    audio_drained_ = false;
    return 0;
  }

  static int allocate_audio(const AudioSpec &spec, int samples,
                            std::size_t maximum, OwnedAudioFrame *output) {
    if (samples < 0)
      return kInvalid;
    const auto format = audio_format(spec.sample_format);
    const int bytes =
        av_samples_get_buffer_size(nullptr, spec.channels, samples, format, 1);
    if (bytes < 0)
      return bytes;
    if (static_cast<std::size_t>(bytes) > maximum)
      return AVERROR(ENOSPC);
    output->bytes.assign(static_cast<std::size_t>(bytes), 0);
    output->spec = spec;
    output->samples_per_channel = static_cast<std::uint64_t>(samples);
    return 0;
  }

  static int finish_audio(const AudioSpec &spec, int samples,
                          OwnedAudioFrame *output) {
    const int bytes = av_samples_get_buffer_size(
        nullptr, spec.channels, samples, audio_format(spec.sample_format), 1);
    if (bytes < 0)
      return bytes;
    output->bytes.resize(static_cast<std::size_t>(bytes));
    output->spec = spec;
    output->samples_per_channel = static_cast<std::uint64_t>(samples);
    return 0;
  }

  static std::size_t packed_video_size(const VideoSpec &spec) noexcept {
    const std::size_t pixels =
        static_cast<std::size_t>(spec.width) * spec.height;
    return spec.pixel_format == PixelFormat::rgba8 ? pixels * 4
                                                   : pixels + pixels / 2;
  }

  static void video_planes(const std::uint8_t *bytes, const VideoSpec &spec,
                           const std::uint8_t *planes[4],
                           int strides[4]) noexcept {
    planes[0] = bytes;
    if (spec.pixel_format == PixelFormat::rgba8) {
      strides[0] = static_cast<int>(spec.width * 4);
      return;
    }
    const std::size_t pixels =
        static_cast<std::size_t>(spec.width) * spec.height;
    planes[1] = bytes + pixels;
    planes[2] = bytes + pixels + pixels / 4;
    strides[0] = static_cast<int>(spec.width);
    strides[1] = static_cast<int>(spec.width / 2);
    strides[2] = static_cast<int>(spec.width / 2);
  }

  static void video_planes(std::uint8_t *bytes, const VideoSpec &spec,
                           std::uint8_t *planes[4], int strides[4]) noexcept {
    const std::uint8_t *readonly[4]{};
    video_planes(static_cast<const std::uint8_t *>(bytes), spec, readonly,
                 strides);
    std::transform(std::begin(readonly), std::end(readonly), planes,
                   [](const std::uint8_t *value) {
                     return const_cast<std::uint8_t *>(value);
                   });
  }

  SwrContext *resampler_ = nullptr;
  SwsContext *scaler_ = nullptr;
  AudioSpec audio_input_{};
  AudioSpec audio_output_{};
  bool has_audio_spec_ = false;
  bool audio_drained_ = false;
};

} // namespace

std::unique_ptr<Backend> make_ffmpeg_backend() noexcept {
  try {
    return std::make_unique<FfmpegBackend>();
  } catch (...) {
    return {};
  }
}

} // namespace muxiva::media
