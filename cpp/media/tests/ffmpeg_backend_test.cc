#include "voxa/media.hpp"

#include <cassert>
#include <cmath>
#include <cstdint>
#include <iostream>
#include <memory>
#include <vector>

int main() {
  using namespace voxa::media;
  Status status;
  auto backend = make_ffmpeg_backend();
  assert(backend);
  auto pipeline = Pipeline::create({}, std::move(backend), &status);
  assert(pipeline && status);
  assert(std::string(pipeline->backend_name()) == "ffmpeg");

  std::vector<std::int16_t> input_samples(480);
  for (std::size_t index = 0; index < input_samples.size(); ++index) {
    input_samples[index] = static_cast<std::int16_t>(
        std::sin(static_cast<double>(index) * 0.1) * 12000.0);
  }
  const auto *bytes =
      reinterpret_cast<const std::uint8_t *>(input_samples.data());
  const AudioSpec input_spec{48000, 1, AudioSampleFormat::i16le};
  const AudioSpec output_spec{16000, 1, AudioSampleFormat::i16le};
  OwnedAudioFrame converted;
  assert(pipeline->convert_audio(
      {bytes, input_samples.size() * sizeof(std::int16_t), input_spec, 480, 0},
      output_spec, &converted));
  assert(converted.samples_per_channel > 0);
  std::uint64_t total_samples = converted.samples_per_channel;
  for (;;) {
    OwnedAudioFrame tail;
    const auto flushed = pipeline->flush_audio(output_spec, &tail);
    if (!flushed)
      std::cerr << flushed.code << " " << flushed.message << '\n';
    assert(flushed);
    if (tail.empty())
      break;
    total_samples += tail.samples_per_channel;
  }
  assert(total_samples >= 159 && total_samples <= 161);

  assert(pipeline->reset());
  std::vector<std::int16_t> stereo(480 * 2, 1000);
  OwnedAudioFrame mono;
  assert(pipeline->convert_audio(
      {reinterpret_cast<const std::uint8_t *>(stereo.data()),
       stereo.size() * sizeof(std::int16_t),
       {48000, 2, AudioSampleFormat::i16le},
       480,
       20'000'000},
      output_spec, &mono));
  assert(mono.spec.channels == 1);

  assert(pipeline->reset());
  std::vector<std::uint8_t> i24(480 * 3, 0);
  OwnedAudioFrame unsupported;
  assert(!pipeline->convert_audio(
      {i24.data(), i24.size(), {48000, 1, AudioSampleFormat::i24le}, 480, 0},
      output_spec, &unsupported));
  assert(pipeline->stats().backend_failures == 1);

  std::vector<std::uint8_t> rgba{
      255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
  };
  OwnedVideoFrame i420;
  assert(pipeline->convert_video(
      {rgba.data(), rgba.size(), {2, 2, PixelFormat::rgba8}, 42},
      {4, 4, PixelFormat::i420}, &i420));
  assert(i420.bytes.size() == 24);
  assert(i420.timestamp_ns == 42);
  OwnedVideoFrame roundtrip;
  assert(pipeline->convert_video(
      {i420.bytes.data(), i420.bytes.size(), i420.spec, 42},
      {2, 2, PixelFormat::rgba8}, &roundtrip));
  assert(roundtrip.bytes.size() == rgba.size());
}
