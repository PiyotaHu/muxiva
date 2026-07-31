use std::sync::atomic::{AtomicU64, Ordering};

use pyo3::{prelude::*, types::PyBytes};
use voxa_types::{
    AudioData, AudioLayout, ByteData, ClockDomain, ClockDomainId, ClockKind, EventData, Extensions,
    Frame, FrameBuffer, FrameHeader, FrameId, FramePayload, FrameType, Lineage, MediaType,
    Metadata, NamespacedName, NodeId, PcmSampleFormat, SchemaVersion, SequenceId, SignalData,
    StreamId, TextData, Timestamp, TraceId, Value, VideoData,
};

use crate::binding_error;

static NEXT_FRAME_ID: AtomicU64 = AtomicU64::new(1);

fn map_core<T, E: std::fmt::Display>(result: Result<T, E>) -> PyResult<T> {
    result.map_err(|error| binding_error("VOXA-PY-FRAME", error.to_string()))
}

fn header(frame_type: FrameType, timestamp_ns: i64, sequence: u64) -> PyResult<FrameHeader> {
    let id = NEXT_FRAME_ID.fetch_add(1, Ordering::Relaxed);
    map_core(FrameHeader::new(
        map_core(FrameId::new(format!("py-frame-{id}")))?,
        Timestamp::from_nanos(timestamp_ns),
        ClockDomain::new(
            map_core(ClockDomainId::new("python.binding"))?,
            ClockKind::Monotonic,
        ),
        SequenceId::new(sequence),
        map_core(StreamId::new("python"))?,
        map_core(TraceId::new(format!("py-trace-{id}")))?,
        frame_type,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    ))
}

fn build(payload: FramePayload, timestamp_ns: i64, sequence: u64) -> PyResult<Frame> {
    let frame_type = payload.frame_type();
    map_core(Frame::new(
        header(frame_type, timestamp_ns, sequence)?,
        payload,
    ))
}

fn type_name(frame: &Frame) -> &'static str {
    match frame {
        Frame::Audio(_) => "audio",
        Frame::Video(_) => "video",
        Frame::Text(_) => "text",
        Frame::Byte(_) => "byte",
        Frame::Signal(_) => "signal",
        Frame::Event(_) => "event",
    }
}

fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.to_string(),
        other => format!("{other:?}"),
    }
}

#[pyclass(frozen, name = "Frame")]
#[derive(Clone)]
pub struct PyFrame {
    pub(crate) inner: Frame,
}

#[pymethods]
impl PyFrame {
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn __repr__(&self) -> String {
        format!(
            "Frame(type='{}', id='{}')",
            type_name(&self.inner),
            self.inner.header().frame_id()
        )
    }
}

#[pyclass(frozen, name = "TextFrame")]
#[derive(Clone)]
pub struct PyTextFrame {
    pub(crate) inner: Frame,
}

#[pymethods]
impl PyTextFrame {
    #[new]
    #[pyo3(signature = (text, *, timestamp_ns=0, sequence=0))]
    fn new(text: String, timestamp_ns: i64, sequence: u64) -> PyResult<Self> {
        Ok(Self {
            inner: build(
                FramePayload::Text(TextData::new(text)),
                timestamp_ns,
                sequence,
            )?,
        })
    }
    #[getter]
    fn text(&self) -> &str {
        self.inner
            .as_text()
            .expect("typed wrapper invariant")
            .data()
            .as_str()
    }
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn as_frame(&self) -> PyFrame {
        PyFrame {
            inner: self.inner.clone(),
        }
    }
}

#[pyclass(frozen, name = "ByteFrame")]
#[derive(Clone)]
pub struct PyByteFrame {
    pub(crate) inner: Frame,
}

#[pymethods]
impl PyByteFrame {
    #[new]
    #[pyo3(signature = (data, *, media_type=None, timestamp_ns=0, sequence=0))]
    fn new(
        data: Vec<u8>,
        media_type: Option<String>,
        timestamp_ns: i64,
        sequence: u64,
    ) -> PyResult<Self> {
        let media_type = media_type
            .map(MediaType::new)
            .transpose()
            .map_err(|e| binding_error("VOXA-PY-FRAME", e.to_string()))?;
        let payload = ByteData::new(FrameBuffer::from_vec(data), media_type);
        Ok(Self {
            inner: build(FramePayload::Byte(payload), timestamp_ns, sequence)?,
        })
    }
    #[getter]
    fn data(&self, py: Python<'_>) -> Py<PyBytes> {
        PyBytes::new(
            py,
            self.inner
                .as_byte()
                .expect("typed wrapper invariant")
                .data()
                .buffer()
                .as_slice(),
        )
        .unbind()
    }
    #[getter]
    fn media_type(&self) -> Option<&str> {
        self.inner
            .as_byte()
            .expect("typed wrapper invariant")
            .data()
            .media_type()
            .map(MediaType::as_str)
    }
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn as_frame(&self) -> PyFrame {
        PyFrame {
            inner: self.inner.clone(),
        }
    }
}

#[pyclass(frozen, name = "AudioFrame")]
#[derive(Clone)]
pub struct PyAudioFrame {
    pub(crate) inner: Frame,
}

fn sample_format(value: &str) -> PyResult<PcmSampleFormat> {
    match value {
        "u8" => Ok(PcmSampleFormat::U8),
        "i16le" => Ok(PcmSampleFormat::I16Le),
        "i24le" => Ok(PcmSampleFormat::I24Le),
        "i32le" => Ok(PcmSampleFormat::I32Le),
        "f32le" => Ok(PcmSampleFormat::F32Le),
        "f64le" => Ok(PcmSampleFormat::F64Le),
        _ => Err(binding_error(
            "VOXA-PY-AUDIO-FORMAT",
            "unsupported PCM sample format",
        )),
    }
}

#[pymethods]
impl PyAudioFrame {
    #[new]
    #[pyo3(signature = (data, sample_rate_hz, channels, samples_per_channel, *, sample_format_name="i16le", layout="interleaved", timestamp_ns=0, sequence=0))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        data: Vec<u8>,
        sample_rate_hz: u32,
        channels: u16,
        samples_per_channel: u64,
        sample_format_name: &str,
        layout: &str,
        timestamp_ns: i64,
        sequence: u64,
    ) -> PyResult<Self> {
        let layout = match layout {
            "interleaved" => AudioLayout::Interleaved,
            "planar" => AudioLayout::Planar,
            _ => {
                return Err(binding_error(
                    "VOXA-PY-AUDIO-LAYOUT",
                    "layout must be interleaved or planar",
                ))
            }
        };
        let audio = map_core(AudioData::new(
            FrameBuffer::from_vec(data),
            sample_rate_hz,
            channels,
            sample_format(sample_format_name)?,
            layout,
            samples_per_channel,
        ))?;
        Ok(Self {
            inner: build(FramePayload::Audio(audio), timestamp_ns, sequence)?,
        })
    }
    #[getter]
    fn data(&self, py: Python<'_>) -> Py<PyBytes> {
        PyBytes::new(
            py,
            self.inner
                .as_audio()
                .expect("typed wrapper invariant")
                .data()
                .buffer()
                .as_slice(),
        )
        .unbind()
    }
    #[getter]
    fn sample_rate_hz(&self) -> u32 {
        self.inner
            .as_audio()
            .expect("typed wrapper invariant")
            .data()
            .sample_rate_hz()
    }
    #[getter]
    fn channels(&self) -> u16 {
        self.inner
            .as_audio()
            .expect("typed wrapper invariant")
            .data()
            .channels()
    }
    #[getter]
    fn samples_per_channel(&self) -> u64 {
        self.inner
            .as_audio()
            .expect("typed wrapper invariant")
            .data()
            .samples_per_channel()
    }
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn as_frame(&self) -> PyFrame {
        PyFrame {
            inner: self.inner.clone(),
        }
    }
}

#[pyclass(frozen, name = "VideoFrame")]
#[derive(Clone)]
pub struct PyVideoFrame {
    pub(crate) inner: Frame,
}

#[pymethods]
impl PyVideoFrame {
    #[new]
    #[pyo3(signature = (data, width, height, *, pixel_format="rgba8", strides=None, timestamp_ns=0, sequence=0))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        pixel_format: &str,
        strides: Option<Vec<usize>>,
        timestamp_ns: i64,
        sequence: u64,
    ) -> PyResult<Self> {
        let buffer = FrameBuffer::from_vec(data);
        let video = match pixel_format {
            "rgba8" => map_core(VideoData::rgba8(
                buffer,
                width,
                height,
                strides
                    .as_deref()
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(width as usize * 4),
            ))?,
            "yuv420p" => {
                let s = strides.ok_or_else(|| {
                    binding_error("VOXA-PY-VIDEO-STRIDE", "yuv420p requires three strides")
                })?;
                if s.len() != 3 {
                    return Err(binding_error(
                        "VOXA-PY-VIDEO-STRIDE",
                        "yuv420p requires three strides",
                    ));
                }
                map_core(VideoData::yuv420p(buffer, width, height, s[0], s[1], s[2]))?
            }
            _ => {
                return Err(binding_error(
                    "VOXA-PY-VIDEO-FORMAT",
                    "pixel_format must be rgba8 or yuv420p",
                ))
            }
        };
        Ok(Self {
            inner: build(FramePayload::Video(video), timestamp_ns, sequence)?,
        })
    }
    #[getter]
    fn data(&self, py: Python<'_>) -> Py<PyBytes> {
        PyBytes::new(
            py,
            self.inner
                .as_video()
                .expect("typed wrapper invariant")
                .data()
                .buffer()
                .as_slice(),
        )
        .unbind()
    }
    #[getter]
    fn width(&self) -> u32 {
        self.inner
            .as_video()
            .expect("typed wrapper invariant")
            .data()
            .width()
    }
    #[getter]
    fn height(&self) -> u32 {
        self.inner
            .as_video()
            .expect("typed wrapper invariant")
            .data()
            .height()
    }
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn as_frame(&self) -> PyFrame {
        PyFrame {
            inner: self.inner.clone(),
        }
    }
}

#[pyclass(frozen, name = "SignalFrame")]
#[derive(Clone)]
pub struct PySignalFrame {
    pub(crate) inner: Frame,
}

#[pymethods]
impl PySignalFrame {
    #[new]
    #[pyo3(signature = (name, payload="", *, source="python.node", schema_version=1, timestamp_ns=0, sequence=0))]
    fn new(
        name: String,
        payload: &str,
        source: &str,
        schema_version: u32,
        timestamp_ns: i64,
        sequence: u64,
    ) -> PyResult<Self> {
        let data = SignalData::new(
            map_core(NamespacedName::new(name))?,
            map_core(SchemaVersion::new(schema_version))?,
            map_core(NodeId::new(source))?,
            Value::String(payload.into()),
        );
        Ok(Self {
            inner: build(FramePayload::Signal(data), timestamp_ns, sequence)?,
        })
    }
    #[getter]
    fn name(&self) -> &str {
        self.inner
            .as_signal()
            .expect("typed wrapper invariant")
            .data()
            .name()
            .as_str()
    }
    #[getter]
    fn payload(&self) -> String {
        value_as_string(
            self.inner
                .as_signal()
                .expect("typed wrapper invariant")
                .data()
                .payload(),
        )
    }
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn as_frame(&self) -> PyFrame {
        PyFrame {
            inner: self.inner.clone(),
        }
    }
}

#[pyclass(frozen, name = "EventFrame")]
#[derive(Clone)]
pub struct PyEventFrame {
    pub(crate) inner: Frame,
}

#[pymethods]
impl PyEventFrame {
    #[new]
    #[pyo3(signature = (topic, payload="", *, source="python.node", schema_version=1, timestamp_ns=0, sequence=0))]
    fn new(
        topic: String,
        payload: &str,
        source: &str,
        schema_version: u32,
        timestamp_ns: i64,
        sequence: u64,
    ) -> PyResult<Self> {
        let data = EventData::new(
            map_core(NamespacedName::new(topic))?,
            map_core(SchemaVersion::new(schema_version))?,
            map_core(NodeId::new(source))?,
            Value::String(payload.into()),
        );
        Ok(Self {
            inner: build(FramePayload::Event(data), timestamp_ns, sequence)?,
        })
    }
    #[getter]
    fn topic(&self) -> &str {
        self.inner
            .as_event()
            .expect("typed wrapper invariant")
            .data()
            .topic()
            .as_str()
    }
    #[getter]
    fn payload(&self) -> String {
        value_as_string(
            self.inner
                .as_event()
                .expect("typed wrapper invariant")
                .data()
                .payload(),
        )
    }
    #[getter]
    fn frame_type(&self) -> &'static str {
        type_name(&self.inner)
    }
    #[getter]
    fn frame_id(&self) -> &str {
        self.inner.header().frame_id().as_str()
    }
    #[getter]
    fn timestamp_ns(&self) -> i64 {
        self.inner.header().timestamp().as_nanos()
    }
    #[getter]
    fn sequence(&self) -> u64 {
        self.inner.header().sequence_id().get()
    }
    fn as_frame(&self) -> PyFrame {
        PyFrame {
            inner: self.inner.clone(),
        }
    }
}

pub(crate) fn frame_to_python(py: Python<'_>, frame: Frame) -> PyResult<Py<PyAny>> {
    Ok(match frame {
        value @ Frame::Audio(_) => Py::new(py, PyAudioFrame { inner: value })?.into_any(),
        value @ Frame::Video(_) => Py::new(py, PyVideoFrame { inner: value })?.into_any(),
        value @ Frame::Text(_) => Py::new(py, PyTextFrame { inner: value })?.into_any(),
        value @ Frame::Byte(_) => Py::new(py, PyByteFrame { inner: value })?.into_any(),
        value @ Frame::Signal(_) => Py::new(py, PySignalFrame { inner: value })?.into_any(),
        value @ Frame::Event(_) => Py::new(py, PyEventFrame { inner: value })?.into_any(),
    })
}

pub(crate) fn extract_frame(value: &Bound<'_, PyAny>) -> PyResult<Frame> {
    if let Ok(value) = value.extract::<PyRef<'_, PyFrame>>() {
        return Ok(value.inner.clone());
    }
    if let Ok(value) = value.extract::<PyRef<'_, PyAudioFrame>>() {
        return Ok(value.inner.clone());
    }
    if let Ok(value) = value.extract::<PyRef<'_, PyVideoFrame>>() {
        return Ok(value.inner.clone());
    }
    if let Ok(value) = value.extract::<PyRef<'_, PyTextFrame>>() {
        return Ok(value.inner.clone());
    }
    if let Ok(value) = value.extract::<PyRef<'_, PyByteFrame>>() {
        return Ok(value.inner.clone());
    }
    if let Ok(value) = value.extract::<PyRef<'_, PySignalFrame>>() {
        return Ok(value.inner.clone());
    }
    if let Ok(value) = value.extract::<PyRef<'_, PyEventFrame>>() {
        return Ok(value.inner.clone());
    }
    Err(binding_error(
        "VOXA-PY-OUTPUT",
        "node output must be a Voxa Frame or a sequence of Frames",
    ))
}
