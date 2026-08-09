use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Instant,
};

use muxiva_core::{
    FrameObservation, FrameObservationDirection, RuntimeObserver, SignalObservation,
    SignalObservationDirection,
};
use muxiva_types::{Frame, FrameHeader, Value};
use serde::Serialize;

const MAX_SESSIONS: usize = 4;
const MAX_ENTRIES: usize = 10_000;
const MAX_PAYLOAD_BYTES: usize = 4 * 1024;
const MAX_SESSION_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Serialize)]
struct TraceEntry {
    ordinal: u64,
    elapsed_ms: u64,
    kind: &'static str,
    direction: &'static str,
    node_id: String,
    port: Option<String>,
    name: String,
    summary: String,
    payload: serde_json::Value,
    payload_truncated: bool,
    frame_id: String,
    sequence: u64,
    stream_id: String,
    trace_id: String,
}

#[derive(Clone, Serialize)]
struct TurnTrace {
    id: String,
    ordinal: u64,
    label: String,
    started_ms: u64,
    entries: Vec<TraceEntry>,
}

struct TraceSession {
    run_id: String,
    status: &'static str,
    started: Instant,
    next_entry: u64,
    next_turn: u64,
    payload_bytes: usize,
    dropped_entries: u64,
    truncated: bool,
    turn_markers: BTreeSet<String>,
    turns: Vec<TurnTrace>,
}

impl TraceSession {
    fn new(run_id: String) -> Self {
        Self {
            run_id,
            status: "running",
            started: Instant::now(),
            next_entry: 0,
            next_turn: 0,
            payload_bytes: 0,
            dropped_entries: 0,
            truncated: false,
            turn_markers: BTreeSet::new(),
            turns: Vec::new(),
        }
    }

    fn append(&mut self, mut entry: TraceEntry, turn_start: Option<String>) {
        if self.next_entry as usize >= MAX_ENTRIES
            || self.payload_bytes >= MAX_SESSION_PAYLOAD_BYTES
        {
            self.dropped_entries = self.dropped_entries.saturating_add(1);
            self.truncated = true;
            return;
        }
        if let Some(marker) = turn_start {
            if self.turn_markers.insert(marker) {
                if self.turns.len() == 1 && self.turns[0].label == "Session flow" {
                    self.turns[0].label = turn_label(&entry.name).into();
                } else {
                    self.next_turn = self.next_turn.saturating_add(1);
                    let carried = self.turns.last_mut().map_or_else(Vec::new, |previous| {
                        let split_at = previous
                            .entries
                            .iter()
                            .rposition(|candidate| {
                                candidate.sequence != entry.sequence
                                    && candidate.trace_id != entry.trace_id
                            })
                            .map_or(0, |index| index + 1);
                        previous.entries.split_off(split_at)
                    });
                    let started_ms = carried
                        .first()
                        .map_or(entry.elapsed_ms, |candidate| candidate.elapsed_ms);
                    self.turns.push(TurnTrace {
                        id: format!("turn-{}", self.next_turn),
                        ordinal: self.next_turn,
                        label: turn_label(&entry.name).into(),
                        started_ms,
                        entries: carried,
                    });
                }
            }
        }
        if self.turns.is_empty() {
            self.next_turn = 1;
            self.turns.push(TurnTrace {
                id: "turn-1".into(),
                ordinal: 1,
                label: "Session flow".into(),
                started_ms: entry.elapsed_ms,
                entries: Vec::new(),
            });
        }
        self.next_entry = self.next_entry.saturating_add(1);
        entry.ordinal = self.next_entry;
        self.payload_bytes = self
            .payload_bytes
            .saturating_add(entry.payload.to_string().len());
        self.turns
            .last_mut()
            .expect("trace has a current turn")
            .entries
            .push(entry);
    }

    fn json(&self) -> serde_json::Value {
        let entries = self
            .turns
            .iter()
            .map(|turn| turn.entries.len())
            .sum::<usize>();
        serde_json::json!({
            "run_id": self.run_id,
            "status": self.status,
            "elapsed_ms": self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            "entries": entries,
            "payload_bytes": self.payload_bytes,
            "dropped_entries": self.dropped_entries,
            "truncated": self.truncated,
            "turns": self.turns,
        })
    }
}

/// Bounded, process-local semantic history for Studio. It intentionally does
/// not persist conversation contents across Studio restarts.
pub struct SemanticTraceStore {
    sessions: Mutex<BTreeMap<String, TraceSession>>,
    active_run_id: Mutex<Option<String>>,
}

impl SemanticTraceStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
            active_run_id: Mutex::new(None),
        }
    }

    pub fn start_session(&self, run_id: &str) {
        *self
            .active_run_id
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(run_id.into());
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        sessions.insert(run_id.into(), TraceSession::new(run_id.into()));
        while sessions.len() > MAX_SESSIONS {
            let Some(oldest) = sessions.keys().next().cloned() else {
                break;
            };
            sessions.remove(&oldest);
        }
    }

    pub fn finish_session(&self, run_id: &str) {
        if let Some(session) = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(run_id)
        {
            session.status = "completed";
        }
    }

    pub fn status_json(&self, requested_run_id: Option<&str>) -> serde_json::Value {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active = self
            .active_run_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let selected = match requested_run_id {
            Some(run_id) => sessions.get(run_id),
            None => active
                .as_deref()
                .and_then(|run_id| sessions.get(run_id))
                .or_else(|| sessions.values().next_back()),
        };
        serde_json::json!({
            "retention": "memory-only",
            "active_run_id": active,
            "limits": {
                "sessions": MAX_SESSIONS,
                "entries": MAX_ENTRIES,
                "payload_bytes": MAX_PAYLOAD_BYTES,
                "session_payload_bytes": MAX_SESSION_PAYLOAD_BYTES,
            },
            "session": selected.map(TraceSession::json),
        })
    }

    fn append_frame(&self, observation: FrameObservation<'_>) {
        let (kind, name, payload, summary) = match observation.frame() {
            Frame::Text(frame) => {
                let text = frame.data().as_str();
                (
                    "text",
                    "text".into(),
                    serde_json::json!(text),
                    summarize(text),
                )
            }
            Frame::Event(frame) => {
                let name = frame.data().topic().as_str().to_owned();
                let payload = value_json(frame.data().payload());
                let summary = summarize(&payload.to_string());
                ("event", name, payload, summary)
            }
            Frame::Signal(frame) => {
                let name = frame.data().name().as_str().to_owned();
                let payload = value_json(frame.data().payload());
                let summary = summarize(&payload.to_string());
                ("signal", name, payload, summary)
            }
            Frame::Audio(_) | Frame::Video(_) | Frame::Byte(_) => return,
        };
        self.append(
            observation.frame().header(),
            kind,
            direction_name(observation.direction()),
            observation.node_id().as_str(),
            Some(observation.port().as_str()),
            name,
            payload,
            summary,
            observation.direction() == FrameObservationDirection::Output,
        );
    }

    fn append_signal(&self, observation: SignalObservation<'_>) {
        let frame = observation.signal();
        let name = frame.data().name().as_str().to_owned();
        let payload = value_json(frame.data().payload());
        let summary = summarize(&payload.to_string());
        self.append(
            frame.header(),
            "signal",
            signal_direction_name(observation.direction()),
            observation.node_id().as_str(),
            observation.port().map(|port| port.as_str()),
            name,
            payload,
            summary,
            observation.direction() == SignalObservationDirection::Output,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn append(
        &self,
        header: &FrameHeader,
        kind: &'static str,
        direction: &'static str,
        node_id: &str,
        port: Option<&str>,
        name: String,
        mut payload: serde_json::Value,
        summary: String,
        output: bool,
    ) {
        let mut payload_truncated = false;
        let serialized = payload.to_string();
        if serialized.len() > MAX_PAYLOAD_BYTES {
            payload = serde_json::json!({
                "preview": truncate(&serialized, MAX_PAYLOAD_BYTES),
                "original_bytes": serialized.len(),
            });
            payload_truncated = true;
        }
        let active = self
            .active_run_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(run_id) = active else {
            return;
        };
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(session) = sessions.get_mut(&run_id) else {
            return;
        };
        let elapsed_ms = session
            .started
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let turn_start = (output && is_turn_start(&name)).then(|| {
            format!(
                "{}|{}|{}|{}",
                name,
                header.sequence_id().get(),
                header.trace_id().as_str(),
                node_id
            )
        });
        session.append(
            TraceEntry {
                ordinal: 0,
                elapsed_ms,
                kind,
                direction,
                node_id: node_id.into(),
                port: port.map(Into::into),
                name,
                summary,
                payload,
                payload_truncated,
                frame_id: header.frame_id().as_str().into(),
                sequence: header.sequence_id().get(),
                stream_id: header.stream_id().as_str().into(),
                trace_id: header.trace_id().as_str().into(),
            },
            turn_start,
        );
    }
}

impl RuntimeObserver for SemanticTraceStore {
    fn observe_frame(&self, observation: FrameObservation<'_>) {
        self.append_frame(observation);
    }

    fn observe_signal(&self, observation: SignalObservation<'_>) {
        self.append_signal(observation);
    }
}

fn direction_name(direction: FrameObservationDirection) -> &'static str {
    match direction {
        FrameObservationDirection::Input => "input",
        FrameObservationDirection::Output => "output",
    }
}

fn signal_direction_name(direction: SignalObservationDirection) -> &'static str {
    match direction {
        SignalObservationDirection::Input => "input",
        SignalObservationDirection::Output => "output",
    }
}

fn is_turn_start(name: &str) -> bool {
    name == "muxiva.turn.started"
        || name == "muxiva.voice.speech.started"
        || name.ends_with(".turn.started")
        || name.ends_with(".speech.started")
}

fn turn_label(name: &str) -> &'static str {
    if name.ends_with(".speech.started") {
        "Speech turn"
    } else {
        "Turn"
    }
}

fn summarize(value: &str) -> String {
    truncate(value.trim(), 180)
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.into();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}…", &value[..end])
}

fn value_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => (*value).into(),
        Value::Integer(value) => (*value).into(),
        Value::Float(value) => serde_json::json!(value.get()),
        Value::String(value) => value.as_ref().into(),
        Value::Bytes(value) => serde_json::json!({
            "bytes": value.len(),
            "preview_hex": value.as_slice().iter().take(32).map(|byte| format!("{byte:02x}")).collect::<String>(),
        }),
        Value::List(values) => values.iter().map(value_json).collect(),
        Value::Map(values) => values
            .iter()
            .map(|(key, value)| (key.to_owned(), value_json(value)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_turn_start, truncate, value_json, SemanticTraceStore, TraceEntry, TraceSession,
    };
    use muxiva_types::{FrameBuffer, Value};

    #[test]
    fn turn_markers_are_explicit_and_payloads_are_safe_json() {
        assert!(is_turn_start("muxiva.voice.speech.started"));
        assert!(is_turn_start("vendor.turn.started"));
        assert!(!is_turn_start("muxiva.voice.speech.stopped"));
        assert_eq!(truncate("你好世界", 7), "你好…");
        assert_eq!(
            value_json(&Value::Bytes(FrameBuffer::from_vec(vec![0xab, 0xcd]))),
            serde_json::json!({"bytes": 2, "preview_hex": "abcd"})
        );
        assert!(SemanticTraceStore::new().status_json(None)["session"].is_null());
    }

    #[test]
    fn session_groups_markers_into_turns_and_deduplicates_the_same_boundary() {
        let mut session = TraceSession::new("run-1".into());
        for (sequence, marker) in [
            (10, Some("speech-10")),
            (10, Some("speech-10")),
            (20, None),
            (20, Some("speech-20")),
        ] {
            session.append(
                TraceEntry {
                    ordinal: 0,
                    elapsed_ms: sequence,
                    kind: "signal",
                    direction: "output",
                    node_id: "vad".into(),
                    port: None,
                    name: "muxiva.voice.speech.started".into(),
                    summary: "{}".into(),
                    payload: serde_json::json!({}),
                    payload_truncated: false,
                    frame_id: format!("frame-{sequence}"),
                    sequence,
                    stream_id: "stream".into(),
                    trace_id: format!("trace-{sequence}"),
                },
                marker.map(Into::into),
            );
        }
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.turns[0].entries.len(), 2);
        assert_eq!(session.turns[1].entries.len(), 2);
        assert_eq!(session.next_entry, 4);
    }
}
