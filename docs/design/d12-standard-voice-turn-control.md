# D12: Standard Voice Turn Control

Status: implemented

## Decision

Muxiva has one provider-neutral decision point for voice turn admission and
barge-in: `builtin.voice_turn_controller`. VAD and ASR adapters report facts;
they do not own cancellation. Agent, TTS, media, and transport adapters react to
one canonical Signal: `muxiva.turn.cancelled`.

```text
audio -> VadAdapter/AsrAdapter -- activity Event --> VoiceTurnController
                              -- final Text -----> VoiceTurnController
device abort -- interrupt.requested Signal -----> VoiceTurnController
                                                   |-- admitted prompt Text -> AgentAdapter
                                                   |-- transcript Text ------> UI encoder
                                                   |-- turn Events ----------> observers/UI
                                                   `-- turn.cancelled Signal -> Agent/TTS/media
```

The controller is framework mechanism plus configurable admission policy. It
does not know Qwen, Xiaozhi, Pi, weather, news, tools, or a product persona.

## Standard protocol

Schema version is `1`.

| Name | Kind | Meaning |
| --- | --- | --- |
| `muxiva.voice.speech.started` | Event | Raw activity began; observation only |
| `muxiva.voice.speech.stopped` | Event | Raw activity ended; observation only |
| `muxiva.turn.interrupt.requested` | Signal | An authoritative device/hardware control requested interruption |
| `muxiva.turn.cancelled` | Signal | Controller committed cancellation through this sequence |
| `muxiva.turn.started` | Event | A meaningful final utterance was admitted |
| `muxiva.turn.utterance.committed` | Event | Final transcript was forwarded |
| `muxiva.turn.utterance.ignored` | Event | Filler/non-speech/tiny transcript was rejected |

Controller decision payloads contain `turn_id`, `generation`, `reason`, and
`controller`. `turn_id` is the admitted input sequence in v1. `generation` is a
monotonic controller-local epoch. Downstream adapters must reject work from a
strictly older sequence/generation, while allowing the prompt that shares the
cancel Signal's sequence.

## Ownership rules

1. `VadAdapter` emits activity Events only. It may support local ducking, but
   ducking must be reversible and must never delete queued media.
2. `AsrAdapter` emits previews, final transcripts, and failure Events. It does
   not fan cancellation out to consumers.
3. `VoiceTurnController` is the only component allowed to convert a transcript
   or authoritative interrupt request into `muxiva.turn.cancelled`.
4. `AgentAdapter` owns model sessions, tools, capability routing, and semantic
   output. Framework `AgentTurnController` owns deadlines, cancellation, stale
   generation suppression, and recovery.
5. `TtsAdapter` accepts text and canonical cancellation, reuses an idle vendor
   session, and drops late PCM from cancelled generations.
6. `MediaController` behavior lives at the final transport/media boundary: a
   canonical cancellation resets queued playback and advances a cancellation
   watermark. Codec framing and paced sending remain transport-specific.
7. `TransportAdapter` maps device protocols to typed Frames. A physical abort
   becomes `muxiva.turn.interrupt.requested`; it does not directly reset every
   downstream component.

## Admission policy

`builtin.voice_turn_controller` normalizes text and rejects only empty text or
an exact normalized entry in the deployment-owned `ignored_utterances` list.
Muxiva Core contains no language-specific filler vocabulary. Deployments may
configure high-confidence Mandarin, English, Spanish, or other filler and
non-speech transcripts without forking provider code.

The admission policy fails open across languages. Unknown final text is always
admitted, even when it is shorter than `minimum_utterance_characters`; that
threshold is only a confidence gate for early cancellation from streaming ASR
previews. `short_utterance_allowlist` can make known short commands interrupt on
their first preview. Consequently an unknown language may lose filler
suppression, but it must never lose the ability to interrupt and create a Turn.

Raw VAD must never cancel because echo, coughs, breathing, and fillers all
produce legitimate activity starts before the final transcript is known.
Meaningful final text commits a new generation, cancels strictly older work,
then emits the prompt. An authoritative hardware abort commits cancellation
immediately without creating a prompt.

## Compatibility and migration

`builtin.audio_vad.signal_out`, Qwen ASR `signal_out`, and
`muxiva.voice.speech.started` cancellation handling remain temporarily for old
graphs. They are deprecated. New graphs must:

1. route ASR `text_out` to `voice_turn_controller.transcript_in`;
2. route VAD `speech_out` to `activity_in` only;
3. route transport abort Signals to `interrupt_in`;
4. fan out only the controller's `signal_out`; and
5. leave provider legacy-interrupt flags disabled.

## Required regression cases

- raw activity while TTS plays does not clear audio;
- `嗯`, `啊`, coughs, empty text, and configured tiny echoes create no turn;
- an allowlisted short command creates exactly one turn;
- meaningful final text emits exactly one cancellation and one prompt;
- a same-sequence prompt survives its cancellation watermark;
- old Agent text, TTS PCM, and transport frames are discarded after cancel;
- a hardware abort clears playback immediately without starting an Agent turn;
- late provider callbacks cannot reopen an older generation;
- paced sender drains the complete accepted TTS stream before stop; and
- reconnect/retry cannot duplicate `playback.started` or a committed turn.
