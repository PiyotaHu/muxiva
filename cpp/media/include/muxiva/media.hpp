#ifndef MUXIVA_MEDIA_HPP
#define MUXIVA_MEDIA_HPP

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace muxiva::media {

struct Status final {
  int code = 0;
  std::string message;

  explicit operator bool() const noexcept { return code == 0; }
  static Status success() noexcept { return {}; }
  static Status failure(int code, const char *message) noexcept;
};

enum class AudioSampleFormat : std::uint8_t {
  u8,
  i16le,
  i24le,
  i32le,
  f32le,
  f64le,
};

enum class PixelFormat : std::uint8_t { rgba8, i420 };

struct AudioSpec final {
  std::uint32_t sample_rate_hz = 0;
  std::uint16_t channels = 0;
  AudioSampleFormat sample_format = AudioSampleFormat::i16le;

  bool operator==(const AudioSpec &other) const noexcept {
    return sample_rate_hz == other.sample_rate_hz &&
           channels == other.channels && sample_format == other.sample_format;
  }
};

struct PackedAudioView final {
  const std::uint8_t *data = nullptr;
  std::size_t size = 0;
  AudioSpec spec;
  std::uint64_t samples_per_channel = 0;
  std::int64_t timestamp_ns = 0;
};

struct OwnedAudioFrame final {
  std::vector<std::uint8_t> bytes;
  AudioSpec spec;
  std::uint64_t samples_per_channel = 0;
  std::int64_t timestamp_ns = 0;

  bool empty() const noexcept { return samples_per_channel == 0; }
};

struct VideoSpec final {
  std::uint32_t width = 0;
  std::uint32_t height = 0;
  PixelFormat pixel_format = PixelFormat::rgba8;

  bool operator==(const VideoSpec &other) const noexcept {
    return width == other.width && height == other.height &&
           pixel_format == other.pixel_format;
  }
};

struct PackedVideoView final {
  const std::uint8_t *data = nullptr;
  std::size_t size = 0;
  VideoSpec spec;
  std::int64_t timestamp_ns = 0;
};

struct OwnedVideoFrame final {
  std::vector<std::uint8_t> bytes;
  VideoSpec spec;
  std::int64_t timestamp_ns = 0;
};

// A Backend is owned by one Pipeline. Calls are serialized by Pipeline and
// must never retain input views after returning.
class Backend {
public:
  virtual ~Backend() = default;
  virtual const char *name() const noexcept = 0;
  virtual int convert_audio(const PackedAudioView &input,
                            const AudioSpec &output_spec,
                            std::size_t max_output_bytes,
                            OwnedAudioFrame *output) noexcept = 0;
  virtual int flush_audio(const AudioSpec &output_spec,
                          std::size_t max_output_bytes,
                          OwnedAudioFrame *output) noexcept = 0;
  virtual int convert_video(const PackedVideoView &input,
                            const VideoSpec &output_spec,
                            std::size_t max_output_bytes,
                            OwnedVideoFrame *output) noexcept = 0;
  virtual void reset() noexcept = 0;
};

struct PipelineConfig final {
  std::size_t max_frame_bytes = 16U * 1024U * 1024U;
  std::chrono::nanoseconds audio_timestamp_tolerance{2'000'000};
};

struct PipelineStats final {
  std::uint64_t audio_frames = 0;
  std::uint64_t video_frames = 0;
  std::uint64_t output_bytes = 0;
  std::uint64_t invalid_rejected = 0;
  std::uint64_t oversized_rejected = 0;
  std::uint64_t busy_rejected = 0;
  std::uint64_t backend_failures = 0;
  std::uint64_t timestamp_discontinuities = 0;
};

class Pipeline final {
public:
  static std::unique_ptr<Pipeline> create(PipelineConfig config,
                                          std::unique_ptr<Backend> backend,
                                          Status *status) noexcept;

  Pipeline(const Pipeline &) = delete;
  Pipeline &operator=(const Pipeline &) = delete;
  ~Pipeline() noexcept;

  Status convert_audio(const PackedAudioView &input,
                       const AudioSpec &output_spec,
                       OwnedAudioFrame *output) noexcept;
  Status flush_audio(const AudioSpec &output_spec,
                     OwnedAudioFrame *output) noexcept;
  Status convert_video(const PackedVideoView &input,
                       const VideoSpec &output_spec,
                       OwnedVideoFrame *output) noexcept;
  Status reset() noexcept;
  const char *backend_name() const noexcept;
  PipelineStats stats() const noexcept;

private:
  struct Impl;
  explicit Pipeline(std::unique_ptr<Impl> impl) noexcept;
  std::unique_ptr<Impl> impl_;
};

// Returns null when Muxiva was built without MUXIVA_ENABLE_FFMPEG.
std::unique_ptr<Backend> make_ffmpeg_backend() noexcept;

} // namespace muxiva::media

#endif
