# Xiaozhi voice regression gate

This suite is the deployment gate for failures reproduced on the Waveshare
ESP32-S3-RLCD-4.2 and Raspberry Pi installation. A voice-path change must not
be deployed unless the deterministic suite passes on the Pi. Changes touching
turn-taking, ASR, Agent, TTS, audio transport, or graph wiring must also pass
the live three-turn case.

Run the deterministic gate from the Muxiva repository root:

```text
python3 examples/xiaozhi-agent/tests/run_voice_regression.py
```

Run the live cloud and WebSocket gate in a maintenance window:

```text
python3 examples/xiaozhi-agent/tests/run_voice_regression.py --live
```

## Locked bad cases

| Case | Required result | Automated coverage |
|---|---|---|
| First words such as “小主人” stutter | packets are prebuffered and paced at real time | gateway pacing test + graph contract |
| Long reply stops after an intermediate phrase | temporary TTS gaps do not complete the Turn | Qwen cascade drain test |
| Final sentence or partial audio disappears | partial PCM is padded; stop follows the last sent packet | gateway roundtrip test |
| Audio is dumped in a burst | 60 ms packets arrive at roughly 60 ms intervals | gateway roundtrip timing assertion |
| Speaker echo cancels its own answer | device AEC plus final-transcript validation prevents unstable partials from cancelling | ASR final-only barge-in tests; physical-board AEC smoke test |
| User cannot interrupt playback | microphone stays live; “闭嘴” and new questions emit barge-in | gateway live-mic + ASR interruption tests |
| “嗯/额”, cough, or mouth noise interrupts playback or opens a Turn | VAD is observational; filler previews/finals produce no cancellation and no prompt | Turn Controller policy + ASR filler tests |
| Long sentence is split too early | final transcript waits for server VAD speech-stopped | ASR pending-final test |
| Old audio leaks after interruption | prior Agent/TTS output and queued gateway audio are cancelled | cascade cancellation + full-duplex test |
| Text flashes but speech is missing | Agent completion is wired to the TTS drain barrier and graph remains acyclic/buildable | graph contract + event encoder test |
| Second/third conversation cannot continue | three consecutive Turns complete and Turn 3 interrupts Turn 2 | live full-duplex test |
| Decimal is read as two separate numbers | `26.2` is normalized to `26点2` | Qwen TTS normalization test |
| Agent stalls or tools return poorly formed text | routing, timeout, weather, web-search, chunking and filtering stay valid | muxiva-pi-agent test suite |

The following still require the physical board smoke test after deployment:
wake word sensitivity at low volume, on-device AEC quality, speaker wiring and
volume, UI state transitions, cat animation, and gallery display duration.
