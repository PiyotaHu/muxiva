# FFmpeg provider review record

| Item | D08 decision |
| --- | --- |
| Locally validated version | Homebrew FFmpeg 8.0.1_2 on macOS arm64 |
| Linked libraries | libavutil, libswresample and libswscale only |
| Default dependency | Disabled; FFmpeg is never fetched or linked by Muxiva Core |
| License/distribution | Muxiva does not redistribute FFmpeg; distributors must audit the license and configuration of their chosen FFmpeg build |
| Buffer ownership | Input views are borrowed for one serialized call; output vectors are owned by the caller |
| Memory bound | Exact input/output validation and per-Pipeline `max_frame_bytes`, capped at 512 MiB |
| Threading | One admitted operation per Pipeline; concurrent calls return `busy` without queueing |
| Audio state | Persistent SwrContext; format changes and post-flush input require explicit reset |
| Video state | Reused SwsContext; timestamps are preserved and buffers are tightly packed |
| Safety evidence | Provider-independent contract under ASan/UBSan and Linux TSan; real backend conversion test when FFmpeg is present |
| Deliberate exclusions | avcodec encode/decode, devices, filters, hardware acceleration, AEC/NS/AGC and playback jitter buffering |

