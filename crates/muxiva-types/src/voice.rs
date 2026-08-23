//! Stable, provider-neutral names for the Muxiva voice turn protocol.
//!
//! Activity events are observations. They must never be interpreted as an
//! authoritative request to delete queued media. Only [`TURN_CANCELLED`],
//! emitted by a turn controller after policy admission, owns cancellation.

/// Raw speech activity began. This is an observation, not a cancel signal.
pub const VOICE_ACTIVITY_STARTED: &str = "muxiva.voice.speech.started";
/// Raw speech activity ended. This is an observation, not a cancel signal.
pub const VOICE_ACTIVITY_STOPPED: &str = "muxiva.voice.speech.stopped";
/// A transport or hardware control requested an immediate interruption.
pub const TURN_INTERRUPT_REQUESTED: &str = "muxiva.turn.interrupt.requested";
/// The turn controller committed cancellation of older generations.
pub const TURN_CANCELLED: &str = "muxiva.turn.cancelled";
/// A meaningful user utterance was admitted as a new turn.
pub const TURN_STARTED: &str = "muxiva.turn.started";
/// A final transcript was admitted and forwarded to the Agent adapter.
pub const TURN_UTTERANCE_COMMITTED: &str = "muxiva.turn.utterance.committed";
/// A filler, non-speech sound, or too-short transcript was rejected.
pub const TURN_UTTERANCE_IGNORED: &str = "muxiva.turn.utterance.ignored";
