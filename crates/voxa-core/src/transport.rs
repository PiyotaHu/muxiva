use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use voxa_types::{
    EventFrame, Extension, ExtensionProducer, ExtensionVisibility, Extensions, Frame,
    FrameDerivation, FrameId, NamespacedName, NodeId, SchemaVersion, SignalFrame, TransformOrigin,
    TurnId, Value, ValueMap,
};

const TURN_EXTENSION: &str = "voxa.transport.turn";
const TURN_CHANGED: &str = "voxa.transport.turn.changed";
const TURN_INTERRUPTED: &str = "voxa.transport.turn.interrupted";
const AUDIO_ENDED: &str = "voxa.transport.audio.ended";
const USER_JOINED: &str = "voxa.transport.user.joined";
const USER_LEFT: &str = "voxa.transport.user.left";
const CONNECTION_CHANGED: &str = "voxa.transport.connection.changed";
pub const RUNTIME_INTERRUPT_SIGNAL: &str = "voxa.runtime.interrupt";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

impl ConnectionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "disconnected" => Some(Self::Disconnected),
            "connecting" => Some(Self::Connecting),
            "connected" => Some(Self::Connected),
            "reconnecting" => Some(Self::Reconnecting),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportSnapshot {
    turn_id: TurnId,
    interrupted: bool,
    audio_ended: bool,
    users: BTreeSet<Box<str>>,
    connection: ConnectionState,
    revision: u64,
}

impl TransportSnapshot {
    pub fn turn_id(&self) -> &TurnId {
        &self.turn_id
    }
    pub const fn interrupted(&self) -> bool {
        self.interrupted
    }
    pub const fn audio_ended(&self) -> bool {
        self.audio_ended
    }
    pub fn users(&self) -> impl Iterator<Item = &str> {
        self.users.iter().map(AsRef::as_ref)
    }
    pub const fn connection(&self) -> ConnectionState {
        self.connection
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlApplyOutcome {
    Applied,
    AlreadyApplied,
    StaleTurn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransportControlError {
    UnknownTopic(Box<str>),
    PayloadNotMap,
    MissingField(&'static str),
    InvalidField(&'static str),
    InvalidTurnId,
    InvalidFrameTurn,
    FrameDerivation(Box<str>),
}

impl fmt::Display for TransportControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTopic(topic) => write!(formatter, "unknown transport topic `{topic}`"),
            Self::PayloadNotMap => formatter.write_str("transport payload must be a Value::Map"),
            Self::MissingField(field) => {
                write!(formatter, "transport payload is missing `{field}`")
            }
            Self::InvalidField(field) => {
                write!(formatter, "transport payload field `{field}` is invalid")
            }
            Self::InvalidTurnId => formatter.write_str("transport turn ID is invalid"),
            Self::InvalidFrameTurn => formatter.write_str("frame has an invalid turn extension"),
            Self::FrameDerivation(message) => {
                write!(formatter, "failed to stamp frame turn: {message}")
            }
        }
    }
}

impl Error for TransportControlError {}

#[derive(Clone)]
pub struct TransportControl {
    state: Arc<RwLock<TransportSnapshot>>,
    stale_sink_drops: Arc<AtomicU64>,
}

impl TransportControl {
    pub fn new(initial_turn: TurnId) -> Self {
        Self {
            state: Arc::new(RwLock::new(TransportSnapshot {
                turn_id: initial_turn,
                interrupted: false,
                audio_ended: false,
                users: BTreeSet::new(),
                connection: ConnectionState::Disconnected,
                revision: 0,
            })),
            stale_sink_drops: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Reads every transport field from one coherent lock-protected snapshot.
    pub fn snapshot(&self) -> TransportSnapshot {
        self.state
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn transition_turn(&self, turn_id: TurnId) -> ControlApplyOutcome {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if state.turn_id == turn_id {
            return ControlApplyOutcome::AlreadyApplied;
        }
        state.turn_id = turn_id;
        state.interrupted = false;
        state.audio_ended = false;
        state.revision = state.revision.saturating_add(1);
        ControlApplyOutcome::Applied
    }

    pub fn interrupt(&self, turn_id: &TurnId) -> ControlApplyOutcome {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if &state.turn_id != turn_id {
            return ControlApplyOutcome::StaleTurn;
        }
        if state.interrupted {
            return ControlApplyOutcome::AlreadyApplied;
        }
        state.interrupted = true;
        state.revision = state.revision.saturating_add(1);
        ControlApplyOutcome::Applied
    }

    /// Atomically invalidates the current turn and opens a fresh runtime turn.
    ///
    /// This is the barge-in primitive used when an acoustic Node cannot know
    /// the Runtime's private TurnId. Frames already stamped with the old turn
    /// become stale immediately at every Sink gate.
    pub fn advance_after_interrupt(&self) -> TurnId {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let next_revision = state.revision.saturating_add(1);
        let turn_id =
            TurnId::new(format!("turn.runtime.{next_revision}")).expect("bounded runtime turn ID");
        state.turn_id = turn_id.clone();
        state.interrupted = false;
        state.audio_ended = false;
        state.revision = next_revision;
        turn_id
    }

    pub fn end_audio(&self, turn_id: &TurnId) -> ControlApplyOutcome {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if &state.turn_id != turn_id {
            return ControlApplyOutcome::StaleTurn;
        }
        if state.audio_ended {
            return ControlApplyOutcome::AlreadyApplied;
        }
        state.audio_ended = true;
        state.revision = state.revision.saturating_add(1);
        ControlApplyOutcome::Applied
    }

    pub fn set_connection(&self, connection: ConnectionState) -> ControlApplyOutcome {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if state.connection == connection {
            return ControlApplyOutcome::AlreadyApplied;
        }
        state.connection = connection;
        state.revision = state.revision.saturating_add(1);
        ControlApplyOutcome::Applied
    }

    pub fn user_join(&self, user_id: impl Into<Box<str>>) -> ControlApplyOutcome {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if !state.users.insert(user_id.into()) {
            return ControlApplyOutcome::AlreadyApplied;
        }
        state.revision = state.revision.saturating_add(1);
        ControlApplyOutcome::Applied
    }

    pub fn user_leave(&self, user_id: &str) -> ControlApplyOutcome {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if !state.users.remove(user_id) {
            return ControlApplyOutcome::AlreadyApplied;
        }
        state.revision = state.revision.saturating_add(1);
        ControlApplyOutcome::Applied
    }

    pub fn apply_signal(
        &self,
        signal: &SignalFrame,
    ) -> Result<ControlApplyOutcome, TransportControlError> {
        self.apply(signal.data().name(), signal.data().payload())
    }

    pub fn apply_event(
        &self,
        event: &EventFrame,
    ) -> Result<ControlApplyOutcome, TransportControlError> {
        self.apply(event.data().topic(), event.data().payload())
    }

    fn apply(
        &self,
        topic: &NamespacedName,
        payload: &Value,
    ) -> Result<ControlApplyOutcome, TransportControlError> {
        let values = value_map(payload)?;
        match topic.as_str() {
            TURN_CHANGED => Ok(self.transition_turn(parse_turn(values)?)),
            TURN_INTERRUPTED => Ok(self.interrupt(&parse_turn(values)?)),
            AUDIO_ENDED => Ok(self.end_audio(&parse_turn(values)?)),
            USER_JOINED => Ok(self.user_join(parse_string(values, "user_id")?)),
            USER_LEFT => Ok(self.user_leave(parse_string(values, "user_id")?)),
            CONNECTION_CHANGED => {
                let value = parse_string(values, "state")?;
                Ok(self.set_connection(
                    ConnectionState::parse(value)
                        .ok_or(TransportControlError::InvalidField("state"))?,
                ))
            }
            other => Err(TransportControlError::UnknownTopic(other.into())),
        }
    }

    /// Derives a new immutable frame with the current private turn extension.
    pub fn stamp_frame(
        &self,
        frame: &Frame,
        new_frame_id: FrameId,
        producer: NodeId,
    ) -> Result<Frame, TransportControlError> {
        let turn_id = self.snapshot().turn_id;
        let key = NamespacedName::new(TURN_EXTENSION).expect("static namespace");
        let version = SchemaVersion::new(1).expect("non-zero constant");
        let mut extensions = frame
            .header()
            .extensions()
            .iter()
            .filter(|extension| !(extension.key() == &key && extension.schema_version() == version))
            .cloned()
            .collect::<Vec<_>>();
        extensions.push(Extension::new(
            key,
            version,
            ExtensionProducer::Core,
            ExtensionVisibility::Private,
            Value::String(Box::from(turn_id.as_str())),
        ));
        let extensions = Extensions::try_from_iter(extensions)
            .map_err(|error| TransportControlError::FrameDerivation(error.to_string().into()))?;
        let origin = TransformOrigin::new(Some(producer), None)
            .map_err(|error| TransportControlError::FrameDerivation(error.to_string().into()))?;
        frame
            .derive(
                FrameDerivation::new(
                    new_frame_id,
                    frame.header().timestamp(),
                    frame.header().sequence_id(),
                    origin,
                    "transport turn stamp",
                )
                .map_err(|error| TransportControlError::FrameDerivation(error.to_string().into()))?
                .with_extensions(extensions),
            )
            .map_err(|error| TransportControlError::FrameDerivation(error.to_string().into()))
    }

    pub fn frame_turn(&self, frame: &Frame) -> Result<Option<TurnId>, TransportControlError> {
        let key = NamespacedName::new(TURN_EXTENSION).expect("static namespace");
        let version = SchemaVersion::new(1).expect("non-zero constant");
        let Some(extension) = frame.header().extensions().get(&key, version) else {
            return Ok(None);
        };
        let Value::String(value) = extension.value() else {
            return Err(TransportControlError::InvalidFrameTurn);
        };
        TurnId::new(value.clone())
            .map(Some)
            .map_err(|_| TransportControlError::InvalidFrameTurn)
    }

    /// Applies the mandatory stale-turn gate immediately before a Sink callback.
    pub fn should_deliver_to_sink(&self, frame: &Frame) -> Result<bool, TransportControlError> {
        let Some(turn_id) = self.frame_turn(frame)? else {
            return Ok(true);
        };
        let deliver = turn_id == self.snapshot().turn_id;
        if !deliver {
            self.stale_sink_drops.fetch_add(1, Ordering::Relaxed);
        }
        Ok(deliver)
    }

    pub fn stale_sink_drops(&self) -> u64 {
        self.stale_sink_drops.load(Ordering::Relaxed)
    }
}

fn value_map(value: &Value) -> Result<&ValueMap, TransportControlError> {
    match value {
        Value::Map(values) => Ok(values),
        _ => Err(TransportControlError::PayloadNotMap),
    }
}

fn parse_string<'a>(
    values: &'a ValueMap,
    key: &'static str,
) -> Result<&'a str, TransportControlError> {
    match values.get(key) {
        Some(Value::String(value)) if !value.is_empty() && value.len() <= 255 => Ok(value),
        Some(_) => Err(TransportControlError::InvalidField(key)),
        None => Err(TransportControlError::MissingField(key)),
    }
}

fn parse_turn(values: &ValueMap) -> Result<TurnId, TransportControlError> {
    TurnId::new(parse_string(values, "turn_id")?).map_err(|_| TransportControlError::InvalidTurnId)
}
