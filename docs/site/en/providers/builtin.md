# Muxiva built-in Nodes

Built-ins are vendor-neutral factories compiled into Muxiva. They are classified by capability even
though they share the Rust runtime binary.

| Node type | Layer | Capability | Contract |
| --- | --- | --- | --- |
| `builtin.audio_resampler` | Media | `audio.resample` | PCM S16LE Audio in and configured-rate Audio out |
| `builtin.audio_vad` | Algorithm | `speech.vad` | PCM Audio in and speech activity Event out |
| `builtin.speech_formatter` | Algorithm | `text.speech_format` | Streaming Markdown Text and cancellation Signal in; TTS-safe plain Text out |
| `builtin.voice_turn_context` | Control | `conversation.turn_context` | Transcript plus speech Event in and turn context Text out |
| `builtin.interval_tick` | Control | `clock.interval` | Periodic Event out |
| `builtin.text_source` | Utility | `text.source` | Configured UTF-8 Text out |
| `builtin.uppercase` | Utility | `text.uppercase` | UTF-8 Text in and uppercase Text out |
| `builtin.text_sink` | Utility | `text.collect` | UTF-8 Text in |
| `builtin.stdout_text_sink` | Utility | `observability.stdout` | UTF-8 Text in and branded stdout logging |

`builtin.demo.*` factories are deterministic architecture previews used by tests and are marked
`utility / demo.voice`. They are not production microphone, ASR, LLM, TTS, or speaker Nodes.

Select any built-in in Studio to inspect its configuration and Port schemas. Media conversion is
explicit: incompatible sample rates should be connected through `builtin.audio_resampler`, not
silently converted inside an Edge.
The generic Node declares `sample_format`, `sample_rate_hz`, and `channels` in explicit `input`
and `output` configuration objects, so 16 kHz input and 48 kHz output are not separate Node types.

`builtin.speech_formatter` lets an Agent's original Markdown branch directly to a rich chat client
while a derived plain-text branch feeds TTS. It strips emphasis markers and bare URLs, keeps link
labels, and replaces streamed fenced-code blocks and Markdown tables with configurable spoken
messages. Its `code_block_message`, `table_message`, and `strip_urls` values are editable in the
Studio Node configuration. The streaming parser retains fence and table state across Text Frames;
an interruption Signal or a new sequence resets that state, so stale unfinished Markdown cannot
silence a later turn.
