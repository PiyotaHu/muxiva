# FFmpeg media provider

## Build

Install FFmpeg development headers and libraries, then configure Voxa:

```sh
cmake -S . -B build/ffmpeg \
  -DVOXA_ENABLE_FFMPEG=ON \
  -DVOXA_FFMPEG_ROOT=/absolute/path/to/ffmpeg
cmake --build build/ffmpeg --target voxa_media
```

Homebrew example:

```sh
brew install ffmpeg
cmake -S . -B build/ffmpeg \
  -DVOXA_ENABLE_FFMPEG=ON \
  -DVOXA_FFMPEG_ROOT="$(brew --prefix ffmpeg)"
```

An installed consumer links `Voxa::media`. When the installed Voxa package was
built with FFmpeg, configure the consumer with the same
`VOXA_FFMPEG_ROOT=/path/to/ffmpeg` so the vendor libraries can be resolved.

## Use

Create one `Pipeline` per logical audio/video stream. Do not call it directly
from an RTC callback if conversion could exceed that callback's deadline;
enqueue the copied frame into Voxa first and convert on a Node/adapter worker.

```cpp
voxa::media::Status status;
auto media = voxa::media::Pipeline::create(
    {}, voxa::media::make_ffmpeg_backend(), &status);
```

Call `flush_audio` until it returns an empty frame before clean end-of-stream.
Call `reset` before reusing the Pipeline with another format or stream. See
`examples/cpp/media-convert` for audio and video conversions.

## Verification

```sh
./scripts/check-media.sh
./scripts/check-media-asan.sh
```

The first command always runs the provider-independent contract test. It also
builds and executes the real FFmpeg backend when the libraries are discoverable.

Voxa does not redistribute FFmpeg. Before shipping binaries, review the license
and build configuration of the FFmpeg distribution you selected. D08 links only
libavutil, libswresample and libswscale; see
[`ffmpeg-review.md`](ffmpeg-review.md) for the completed dependency review.
