#include "voxa/media.hpp"

#include <atomic>
#include <cassert>
#include <condition_variable>
#include <cstdint>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

namespace {

std::size_t width(voxa::media::AudioSampleFormat format) {
  using Format = voxa::media::AudioSampleFormat;
  if (format == Format::u8)
    return 1;
  if (format == Format::i16le)
    return 2;
  if (format == Format::i24le)
    return 3;
  if (format == Format::f64le)
    return 8;
  return 4;
}

struct BlockState final {
  std::mutex mutex;
  std::condition_variable cv;
  bool block = false;
  bool entered = false;
};

class DeterministicBackend final : public voxa::media::Backend {
public:
  explicit DeterministicBackend(std::shared_ptr<BlockState> state = {})
      : state_(std::move(state)) {}

  const char *name() const noexcept override { return "deterministic"; }

  int convert_audio(const voxa::media::PackedAudioView &input,
                    const voxa::media::AudioSpec &output_spec,
                    std::size_t maximum,
                    voxa::media::OwnedAudioFrame *output) noexcept override {
    if (state_) {
      std::unique_lock<std::mutex> lock(state_->mutex);
      state_->entered = true;
      state_->cv.notify_all();
      state_->cv.wait(lock, [&] { return !state_->block; });
    }
    const auto samples = input.samples_per_channel *
                         output_spec.sample_rate_hz / input.spec.sample_rate_hz;
    const auto bytes = static_cast<std::size_t>(samples) *
                       output_spec.channels * width(output_spec.sample_format);
    if (bytes > maximum)
      return -9;
    output->bytes.assign(bytes, 17);
    output->spec = output_spec;
    output->samples_per_channel = samples;
    return 0;
  }

  int flush_audio(const voxa::media::AudioSpec &output_spec, std::size_t,
                  voxa::media::OwnedAudioFrame *output) noexcept override {
    output->bytes.clear();
    output->spec = output_spec;
    output->samples_per_channel = 0;
    return 0;
  }

  int convert_video(const voxa::media::PackedVideoView &,
                    const voxa::media::VideoSpec &output_spec,
                    std::size_t maximum,
                    voxa::media::OwnedVideoFrame *output) noexcept override {
    const auto pixels =
        static_cast<std::size_t>(output_spec.width) * output_spec.height;
    const auto bytes =
        output_spec.pixel_format == voxa::media::PixelFormat::rgba8
            ? pixels * 4
            : pixels + pixels / 2;
    if (bytes > maximum)
      return -9;
    output->bytes.assign(bytes, 23);
    output->spec = output_spec;
    return 0;
  }

  void reset() noexcept override {}

private:
  std::shared_ptr<BlockState> state_;
};

} // namespace

int main() {
  using namespace voxa::media;
  PipelineConfig config;
  config.max_frame_bytes = 4096;
  config.audio_timestamp_tolerance = std::chrono::milliseconds(2);
  Status status;
  auto pipeline = Pipeline::create(
      config, std::make_unique<DeterministicBackend>(), &status);
  assert(pipeline && status);
  assert(std::string(pipeline->backend_name()) == "deterministic");

  const AudioSpec input_spec{48000, 1, AudioSampleFormat::i16le};
  const AudioSpec output_spec{16000, 1, AudioSampleFormat::i16le};
  std::vector<std::uint8_t> pcm(960, 1);
  OwnedAudioFrame first;
  assert(pipeline->convert_audio({pcm.data(), pcm.size(), input_spec, 480, 0},
                                 output_spec, &first));
  assert(first.samples_per_channel == 160);
  assert(first.bytes.size() == 320);
  assert(first.timestamp_ns == 0);

  OwnedAudioFrame second;
  assert(pipeline->convert_audio(
      {pcm.data(), pcm.size(), input_spec, 480, 11'000'000}, output_spec,
      &second));
  assert(second.timestamp_ns == 10'000'000);

  OwnedAudioFrame discontinuous;
  assert(pipeline->convert_audio(
      {pcm.data(), pcm.size(), input_spec, 480, 30'000'000}, output_spec,
      &discontinuous));
  assert(discontinuous.timestamp_ns == 30'000'000);
  assert(pipeline->stats().timestamp_discontinuities == 1);

  OwnedAudioFrame invalid;
  assert(!pipeline->convert_audio(
      {pcm.data(), pcm.size() - 1, input_spec, 480, 40'000'000}, output_spec,
      &invalid));
  assert(pipeline->stats().invalid_rejected == 1);

  std::vector<std::uint8_t> rgba(64, 9);
  OwnedVideoFrame i420;
  assert(pipeline->convert_video(
      {rgba.data(), rgba.size(), {4, 4, PixelFormat::rgba8}, 7},
      {4, 4, PixelFormat::i420}, &i420));
  assert(i420.bytes.size() == 24);
  assert(i420.timestamp_ns == 7);

  OwnedAudioFrame tail;
  assert(pipeline->flush_audio(output_spec, &tail));
  assert(tail.empty());
  assert(!pipeline->convert_audio(
      {pcm.data(), pcm.size(), input_spec, 480, 40'000'000}, output_spec,
      &tail));
  assert(pipeline->reset());
  assert(pipeline->convert_audio(
      {pcm.data(), pcm.size(), input_spec, 480, 40'000'000}, output_spec,
      &tail));

  PipelineConfig small_config;
  small_config.max_frame_bytes = 32;
  auto small = Pipeline::create(
      small_config, std::make_unique<DeterministicBackend>(), &status);
  OwnedVideoFrame rejected;
  assert(!small->convert_video(
      {rgba.data(), rgba.size(), {4, 4, PixelFormat::rgba8}, 0},
      {4, 4, PixelFormat::i420}, &rejected));
  assert(small->stats().oversized_rejected == 1);

  auto block = std::make_shared<BlockState>();
  block->block = true;
  auto concurrent = Pipeline::create(
      config, std::make_unique<DeterministicBackend>(block), &status);
  std::atomic<bool> completed{false};
  std::thread owner([&] {
    OwnedAudioFrame output;
    completed = static_cast<bool>(concurrent->convert_audio(
        {pcm.data(), pcm.size(), input_spec, 480, 0}, output_spec, &output));
  });
  {
    std::unique_lock<std::mutex> lock(block->mutex);
    block->cv.wait(lock, [&] { return block->entered; });
  }
  OwnedAudioFrame busy;
  assert(!concurrent->convert_audio(
      {pcm.data(), pcm.size(), input_spec, 480, 0}, output_spec, &busy));
  assert(concurrent->stats().busy_rejected == 1);
  {
    std::lock_guard<std::mutex> lock(block->mutex);
    block->block = false;
  }
  block->cv.notify_all();
  owner.join();
  assert(completed);
}
