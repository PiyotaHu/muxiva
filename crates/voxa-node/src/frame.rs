use napi::{bindgen_prelude::Buffer, Error, Result, Status};
use napi_derive::napi;
use voxa_types::{
    AudioData, AudioLayout, ByteData, ClockDomain, ClockDomainId, ClockKind, EventData, Extensions,
    Frame as RustFrame, FrameBuffer, FrameHeader, FrameId, FramePayload, FrameType, Lineage,
    MediaType, Metadata, NamespacedName, NodeId, PcmSampleFormat, SchemaVersion, SequenceId,
    SignalData, StreamId, TextData, Timestamp, TraceId, Value, VideoData,
};

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::new(Status::InvalidArg, error.to_string())
}

fn header(frame_type: FrameType, sequence: i64) -> Result<FrameHeader> {
    let sequence = u64::try_from(sequence).map_err(|_| invalid("sequence must be non-negative"))?;
    FrameHeader::new(
        FrameId::new(format!("node-frame-{sequence}")).map_err(invalid)?,
        Timestamp::from_nanos(0),
        ClockDomain::new(
            ClockDomainId::new("node.binding").map_err(invalid)?,
            ClockKind::Monotonic,
        ),
        SequenceId::new(sequence),
        StreamId::new("node-stream").map_err(invalid)?,
        TraceId::new("node-trace").map_err(invalid)?,
        frame_type,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .map_err(invalid)
}

fn checked_frame(payload: FramePayload, sequence: i64) -> Result<RustFrame> {
    RustFrame::new(header(payload.frame_type(), sequence)?, payload).map_err(invalid)
}

pub(crate) fn owned_text_frame(text: String, sequence: i64) -> Result<RustFrame> {
    checked_frame(FramePayload::Text(TextData::new(text)), sequence)
}

pub(crate) fn owned_signal_frame(
    payload: String,
    sequence: i64,
) -> Result<voxa_types::SignalFrame> {
    let data = SignalData::new(
        NamespacedName::new("node.signal").map_err(invalid)?,
        SchemaVersion::new(1).map_err(invalid)?,
        NodeId::new("node-binding").map_err(invalid)?,
        Value::String(payload.into()),
    );
    let frame = checked_frame(FramePayload::Signal(data), sequence)?;
    match frame {
        RustFrame::Signal(frame) => Ok(frame),
        _ => unreachable!("signal payload constructs signal frame"),
    }
}

pub(crate) fn owned_event_frame(payload: String, sequence: i64) -> Result<voxa_types::EventFrame> {
    let data = EventData::new(
        NamespacedName::new("node.event").map_err(invalid)?,
        SchemaVersion::new(1).map_err(invalid)?,
        NodeId::new("node-binding").map_err(invalid)?,
        Value::String(payload.into()),
    );
    let frame = checked_frame(FramePayload::Event(data), sequence)?;
    match frame {
        RustFrame::Event(frame) => Ok(frame),
        _ => unreachable!("event payload constructs event frame"),
    }
}

fn kind(frame: &RustFrame) -> &'static str {
    match frame.frame_type() {
        FrameType::Audio => "audio",
        FrameType::Video => "video",
        FrameType::Text => "text",
        FrameType::Byte => "byte",
        FrameType::Signal => "signal",
        FrameType::Event => "event",
    }
}

#[napi]
pub struct Frame {
    pub(crate) inner: RustFrame,
}

#[napi]
impl Frame {
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn copy(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[napi]
pub struct TextFrame {
    inner: RustFrame,
}
#[napi]
impl TextFrame {
    #[napi(constructor)]
    pub fn new(text: String, sequence: i64) -> Result<Self> {
        Ok(Self {
            inner: checked_frame(FramePayload::Text(TextData::new(text)), sequence)?,
        })
    }
    #[napi(getter)]
    pub fn text(&self) -> String {
        self.inner
            .as_text()
            .expect("typed")
            .data()
            .as_str()
            .to_owned()
    }
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn as_frame(&self) -> Frame {
        Frame {
            inner: self.inner.clone(),
        }
    }
}

#[napi]
pub struct ByteFrame {
    inner: RustFrame,
}
#[napi]
impl ByteFrame {
    #[napi(constructor)]
    pub fn new(bytes: Buffer, media_type: Option<String>, sequence: i64) -> Result<Self> {
        let media_type = media_type
            .map(MediaType::new)
            .transpose()
            .map_err(invalid)?;
        let data = ByteData::new(FrameBuffer::from_vec(bytes.to_vec()), media_type);
        Ok(Self {
            inner: checked_frame(FramePayload::Byte(data), sequence)?,
        })
    }
    #[napi(getter)]
    pub fn bytes(&self) -> Buffer {
        self.inner
            .as_byte()
            .expect("typed")
            .data()
            .buffer()
            .as_slice()
            .to_vec()
            .into()
    }
    #[napi(getter)]
    pub fn media_type(&self) -> Option<String> {
        self.inner
            .as_byte()
            .expect("typed")
            .data()
            .media_type()
            .map(|v| v.as_str().to_owned())
    }
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn as_frame(&self) -> Frame {
        Frame {
            inner: self.inner.clone(),
        }
    }
}

fn sample_format(value: &str) -> Result<PcmSampleFormat> {
    match value {
        "u8" => Ok(PcmSampleFormat::U8),
        "i16le" => Ok(PcmSampleFormat::I16Le),
        "i24le" => Ok(PcmSampleFormat::I24Le),
        "i32le" => Ok(PcmSampleFormat::I32Le),
        "f32le" => Ok(PcmSampleFormat::F32Le),
        "f64le" => Ok(PcmSampleFormat::F64Le),
        _ => Err(invalid("unsupported PCM sample format")),
    }
}

#[napi]
pub struct AudioFrame {
    inner: RustFrame,
}
#[napi]
impl AudioFrame {
    #[napi(constructor)]
    pub fn new(
        bytes: Buffer,
        sample_rate_hz: u32,
        channels: u32,
        format: String,
        planar: bool,
        samples_per_channel: i64,
        sequence: i64,
    ) -> Result<Self> {
        let channels = u16::try_from(channels).map_err(|_| invalid("channels exceeds u16"))?;
        let samples = u64::try_from(samples_per_channel)
            .map_err(|_| invalid("samplesPerChannel must be positive"))?;
        let data = AudioData::new(
            FrameBuffer::from_vec(bytes.to_vec()),
            sample_rate_hz,
            channels,
            sample_format(&format)?,
            if planar {
                AudioLayout::Planar
            } else {
                AudioLayout::Interleaved
            },
            samples,
        )
        .map_err(invalid)?;
        Ok(Self {
            inner: checked_frame(FramePayload::Audio(data), sequence)?,
        })
    }
    #[napi(getter)]
    pub fn bytes(&self) -> Buffer {
        self.inner
            .as_audio()
            .expect("typed")
            .data()
            .buffer()
            .as_slice()
            .to_vec()
            .into()
    }
    #[napi(getter)]
    pub fn sample_rate_hz(&self) -> u32 {
        self.inner
            .as_audio()
            .expect("typed")
            .data()
            .sample_rate_hz()
    }
    #[napi(getter)]
    pub fn channels(&self) -> u32 {
        u32::from(self.inner.as_audio().expect("typed").data().channels())
    }
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn as_frame(&self) -> Frame {
        Frame {
            inner: self.inner.clone(),
        }
    }
}

#[napi]
pub struct VideoFrame {
    inner: RustFrame,
}
#[napi]
impl VideoFrame {
    #[napi(constructor)]
    pub fn new_rgba8(
        bytes: Buffer,
        width: u32,
        height: u32,
        stride: u32,
        sequence: i64,
    ) -> Result<Self> {
        let data = VideoData::rgba8(
            FrameBuffer::from_vec(bytes.to_vec()),
            width,
            height,
            stride as usize,
        )
        .map_err(invalid)?;
        Ok(Self {
            inner: checked_frame(FramePayload::Video(data), sequence)?,
        })
    }
    #[napi(getter)]
    pub fn bytes(&self) -> Buffer {
        self.inner
            .as_video()
            .expect("typed")
            .data()
            .buffer()
            .as_slice()
            .to_vec()
            .into()
    }
    #[napi(getter)]
    pub fn width(&self) -> u32 {
        self.inner.as_video().expect("typed").data().width()
    }
    #[napi(getter)]
    pub fn height(&self) -> u32 {
        self.inner.as_video().expect("typed").data().height()
    }
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn as_frame(&self) -> Frame {
        Frame {
            inner: self.inner.clone(),
        }
    }
}

fn namespaced(value: String) -> Result<NamespacedName> {
    NamespacedName::new(value).map_err(invalid)
}
fn node_id(value: String) -> Result<NodeId> {
    NodeId::new(value).map_err(invalid)
}
fn schema(value: u32) -> Result<SchemaVersion> {
    SchemaVersion::new(value).map_err(invalid)
}

#[napi]
pub struct SignalFrame {
    inner: RustFrame,
}
#[napi]
impl SignalFrame {
    #[napi(constructor)]
    pub fn new(
        name: String,
        source: String,
        schema_version: u32,
        payload_json: String,
        sequence: i64,
    ) -> Result<Self> {
        let data = SignalData::new(
            namespaced(name)?,
            schema(schema_version)?,
            node_id(source)?,
            Value::String(payload_json.into()),
        );
        Ok(Self {
            inner: checked_frame(FramePayload::Signal(data), sequence)?,
        })
    }
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.inner
            .as_signal()
            .expect("typed")
            .data()
            .name()
            .as_str()
            .to_owned()
    }
    #[napi(getter)]
    pub fn payload_json(&self) -> String {
        match self.inner.as_signal().expect("typed").data().payload() {
            Value::String(v) => v.to_string(),
            _ => unreachable!(),
        }
    }
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn as_frame(&self) -> Frame {
        Frame {
            inner: self.inner.clone(),
        }
    }
}

#[napi]
pub struct EventFrame {
    inner: RustFrame,
}
#[napi]
impl EventFrame {
    #[napi(constructor)]
    pub fn new(
        topic: String,
        source: String,
        schema_version: u32,
        payload_json: String,
        sequence: i64,
    ) -> Result<Self> {
        let data = EventData::new(
            namespaced(topic)?,
            schema(schema_version)?,
            node_id(source)?,
            Value::String(payload_json.into()),
        );
        Ok(Self {
            inner: checked_frame(FramePayload::Event(data), sequence)?,
        })
    }
    #[napi(getter)]
    pub fn topic(&self) -> String {
        self.inner
            .as_event()
            .expect("typed")
            .data()
            .topic()
            .as_str()
            .to_owned()
    }
    #[napi(getter)]
    pub fn payload_json(&self) -> String {
        match self.inner.as_event().expect("typed").data().payload() {
            Value::String(v) => v.to_string(),
            _ => unreachable!(),
        }
    }
    #[napi(getter)]
    pub fn kind(&self) -> &'static str {
        kind(&self.inner)
    }
    #[napi(getter)]
    pub fn sequence(&self) -> i64 {
        self.inner.header().sequence_id().get() as i64
    }
    #[napi]
    pub fn as_frame(&self) -> Frame {
        Frame {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_is_owned_and_immutable() {
        let f = TextFrame::new("hello".into(), 1).unwrap();
        assert_eq!(f.text(), "hello");
        assert_eq!(f.kind(), "text");
    }
    #[test]
    fn audio_validates_exact_length() {
        assert!(
            AudioFrame::new(vec![0; 3].into(), 48_000, 1, "i16le".into(), false, 2, 0).is_err()
        );
    }
}
