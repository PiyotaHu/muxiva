use muxiva_types::{
    AudioData, AudioLayout, ByteData, ClockDomain, ClockDomainId, ClockKind, EventData, Extensions,
    Frame as RustFrame, FrameBuffer, FrameHeader, FrameId, FramePayload, FrameType, Lineage,
    MediaType, Metadata, NamespacedName, NodeId, PcmSampleFormat, PixelFormat, SchemaVersion,
    SequenceId, SignalData, StreamId, TextData, Timestamp, TraceId, Value, VideoData, VideoLayout,
};
use napi::{bindgen_prelude::Buffer, Error, Result, Status};
use napi_derive::napi;

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

pub(crate) fn frame_to_wire(frame: &RustFrame) -> Result<serde_json::Value> {
    let sequence = frame.header().sequence_id().get();
    let value = match frame {
        RustFrame::Text(frame) => serde_json::json!({
            "kind": "text", "sequence": sequence, "text": frame.data().as_str()
        }),
        RustFrame::Byte(frame) => serde_json::json!({
            "kind": "byte", "sequence": sequence,
            "bytes": frame.data().buffer().as_slice(),
            "mediaType": frame.data().media_type().map(|value| value.as_str())
        }),
        RustFrame::Audio(frame) => {
            let data = frame.data();
            let format = match data.sample_format() {
                PcmSampleFormat::U8 => "u8",
                PcmSampleFormat::I16Le => "i16le",
                PcmSampleFormat::I24Le => "i24le",
                PcmSampleFormat::I32Le => "i32le",
                PcmSampleFormat::F32Le => "f32le",
                PcmSampleFormat::F64Le => "f64le",
            };
            serde_json::json!({
                "kind": "audio", "sequence": sequence,
                "bytes": data.buffer().as_slice(), "sampleRateHz": data.sample_rate_hz(),
                "channels": data.channels(), "format": format,
                "planar": data.layout() == AudioLayout::Planar,
                "samplesPerChannel": data.samples_per_channel()
            })
        }
        RustFrame::Video(frame) => {
            let data = frame.data();
            if data.pixel_format() != PixelFormat::Rgba8 {
                return Err(invalid(
                    "the TypeScript graph wire protocol currently requires RGBA8 video",
                ));
            }
            let VideoLayout::Rgba8 { plane } = data.layout() else {
                unreachable!()
            };
            serde_json::json!({
                "kind": "video", "sequence": sequence, "pixelFormat": "rgba8",
                "bytes": data.buffer().as_slice(), "width": data.width(),
                "height": data.height(), "stride": plane.stride()
            })
        }
        RustFrame::Signal(_) | RustFrame::Event(_) => {
            return Err(invalid(
                "signal and event frames are control-plane values, not graph port payloads",
            ));
        }
    };
    Ok(value)
}

pub(crate) fn frame_from_wire(value: &serde_json::Value) -> Result<RustFrame> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("frame must be an object"))?;
    let field = |name: &str| {
        object
            .get(name)
            .ok_or_else(|| invalid(format!("frame.{name} is required")))
    };
    let sequence = field("sequence")?
        .as_i64()
        .ok_or_else(|| invalid("frame.sequence must be an integer"))?;
    let payload = match field("kind")?.as_str() {
        Some("text") => FramePayload::Text(TextData::new(
            field("text")?
                .as_str()
                .ok_or_else(|| invalid("frame.text must be a string"))?,
        )),
        Some("byte") => {
            let bytes: Vec<u8> =
                serde_json::from_value(field("bytes")?.clone()).map_err(invalid)?;
            let media_type = object
                .get("mediaType")
                .and_then(serde_json::Value::as_str)
                .map(MediaType::new)
                .transpose()
                .map_err(invalid)?;
            FramePayload::Byte(ByteData::new(FrameBuffer::from_vec(bytes), media_type))
        }
        Some("audio") => {
            let bytes: Vec<u8> =
                serde_json::from_value(field("bytes")?.clone()).map_err(invalid)?;
            let rate = u32::try_from(
                field("sampleRateHz")?
                    .as_u64()
                    .ok_or_else(|| invalid("frame.sampleRateHz must be an integer"))?,
            )
            .map_err(invalid)?;
            let channels = u16::try_from(
                field("channels")?
                    .as_u64()
                    .ok_or_else(|| invalid("frame.channels must be an integer"))?,
            )
            .map_err(invalid)?;
            let samples = field("samplesPerChannel")?
                .as_u64()
                .ok_or_else(|| invalid("frame.samplesPerChannel must be an integer"))?;
            let format = sample_format(
                field("format")?
                    .as_str()
                    .ok_or_else(|| invalid("frame.format must be a string"))?,
            )?;
            let layout = if object
                .get("planar")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                AudioLayout::Planar
            } else {
                AudioLayout::Interleaved
            };
            FramePayload::Audio(
                AudioData::new(
                    FrameBuffer::from_vec(bytes),
                    rate,
                    channels,
                    format,
                    layout,
                    samples,
                )
                .map_err(invalid)?,
            )
        }
        Some("video") => {
            if object
                .get("pixelFormat")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("rgba8")
                != "rgba8"
            {
                return Err(invalid(
                    "only rgba8 video is supported by the TypeScript graph wire protocol",
                ));
            }
            let bytes: Vec<u8> =
                serde_json::from_value(field("bytes")?.clone()).map_err(invalid)?;
            let width = u32::try_from(
                field("width")?
                    .as_u64()
                    .ok_or_else(|| invalid("frame.width must be an integer"))?,
            )
            .map_err(invalid)?;
            let height = u32::try_from(
                field("height")?
                    .as_u64()
                    .ok_or_else(|| invalid("frame.height must be an integer"))?,
            )
            .map_err(invalid)?;
            let stride = usize::try_from(
                field("stride")?
                    .as_u64()
                    .ok_or_else(|| invalid("frame.stride must be an integer"))?,
            )
            .map_err(invalid)?;
            FramePayload::Video(
                VideoData::rgba8(FrameBuffer::from_vec(bytes), width, height, stride)
                    .map_err(invalid)?,
            )
        }
        _ => return Err(invalid("frame.kind must be text, byte, audio, or video")),
    };
    checked_frame(payload, sequence)
}

pub(crate) fn owned_signal_frame(
    payload: String,
    sequence: i64,
) -> Result<muxiva_types::SignalFrame> {
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

pub(crate) fn owned_event_frame(
    payload: String,
    sequence: i64,
) -> Result<muxiva_types::EventFrame> {
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
