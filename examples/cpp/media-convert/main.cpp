#include <voxa/media.hpp>

#include <cstdint>
#include <iostream>
#include <vector>

int main() {
  using namespace voxa::media;
  Status status;
  auto pipeline = Pipeline::create({}, make_ffmpeg_backend(), &status);
  if (!pipeline) {
    std::cerr << "FFmpeg provider is unavailable: " << status.message << '\n';
    return 1;
  }

  std::vector<std::int16_t> pcm(480, 1000);
  const auto *bytes = reinterpret_cast<const std::uint8_t *>(pcm.data());
  OwnedAudioFrame audio;
  status =
      pipeline->convert_audio({bytes,
                               pcm.size() * sizeof(std::int16_t),
                               {48000, 1, AudioSampleFormat::i16le},
                               480,
                               0},
                              {16000, 1, AudioSampleFormat::i16le}, &audio);
  if (!status) {
    std::cerr << status.code << ": " << status.message << '\n';
    return 1;
  }
  std::uint64_t audio_samples = audio.samples_per_channel;
  for (;;) {
    OwnedAudioFrame tail;
    status = pipeline->flush_audio({16000, 1, AudioSampleFormat::i16le}, &tail);
    if (!status) {
      std::cerr << status.code << ": " << status.message << '\n';
      return 1;
    }
    if (tail.empty())
      break;
    audio_samples += tail.samples_per_channel;
  }

  std::vector<std::uint8_t> rgba(4 * 4 * 4, 255);
  OwnedVideoFrame video;
  status = pipeline->convert_video(
      {rgba.data(), rgba.size(), {4, 4, PixelFormat::rgba8}, 0},
      {4, 4, PixelFormat::i420}, &video);
  if (!status) {
    std::cerr << status.code << ": " << status.message << '\n';
    return 1;
  }

  std::cout << pipeline->backend_name() << ": audio=" << audio_samples
            << " samples, video=" << video.bytes.size() << " bytes\n";
}
