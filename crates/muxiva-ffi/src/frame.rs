use std::{ffi::c_void, mem, ptr::NonNull, sync::Arc};

use libloading::Library;

use muxiva_types::{
    AudioData, AudioLayout, ByteData, ClockDomain, ClockDomainId, ClockKind, Extensions, Frame,
    FrameBuffer, FrameHeader as RustHeader, FrameId, FramePayload as RustPayload, FrameType,
    Lineage, MediaType, Metadata, PcmSampleFormat, PixelFormat, SequenceId, StreamId, TextData,
    Timestamp, TraceId, VideoData, VideoLayout,
};

use crate::{
    abi::{self, FramePayload, FrameView, OwnedNamedFrameView, StrView},
    error::FfiError,
};

#[derive(Clone)]
pub struct OwnedFrame {
    pub header: OwnedHeader,
    pub payload: OwnedPayload,
}

#[derive(Clone)]
pub struct OwnedHeader {
    pub frame_type: u32,
    pub clock_kind: u32,
    pub timestamp_ns: i64,
    pub sequence_id: u64,
    pub frame_id: String,
    pub clock_domain_id: String,
    pub stream_id: String,
    pub trace_id: String,
}

#[derive(Clone)]
pub enum OwnedPayload {
    Audio {
        bytes: Vec<u8>,
        sample_rate_hz: u32,
        channels: u16,
        sample_format: u16,
        layout: u32,
        samples_per_channel: u64,
    },
    Video {
        bytes: Vec<u8>,
        width: u32,
        height: u32,
        pixel_format: u32,
        plane_count: u32,
    },
    Text(String),
    Byte {
        bytes: Vec<u8>,
        media_type: Option<String>,
    },
    Signal(Vec<u8>),
    Event(Vec<u8>),
}

pub fn copy_frame(pointer: *const FrameView) -> Result<OwnedFrame, FfiError> {
    if !abi::aligned(pointer) {
        return Err(invalid("frame pointer is null or unaligned"));
    }
    let prefix = abi::read_copy(pointer.cast::<abi::AbiPrefix>())
        .ok_or_else(|| invalid("frame header prefix is not readable"))?;
    let expected = u32::try_from(mem::size_of::<abi::FrameHeader>()).unwrap_or(u32::MAX);
    if prefix.abi_version != abi::ABI_VERSION || prefix.struct_size != expected {
        return Err(FfiError::abi(
            "frame header version or size does not match v1",
        ));
    }
    let frame = abi::read_copy(pointer).ok_or_else(|| invalid("frame view is not readable"))?;
    if frame.header.reserved != [0; 4] {
        return Err(invalid("frame header reserved fields must be zero"));
    }
    if !(1..=3).contains(&frame.header.clock_kind) {
        return Err(invalid("unknown clock kind"));
    }
    let header = OwnedHeader {
        frame_type: frame.header.frame_type,
        clock_kind: frame.header.clock_kind,
        timestamp_ns: frame.header.timestamp_ns,
        sequence_id: frame.header.sequence_id,
        frame_id: abi::copy_str(frame.header.frame_id, true).map_err(invalid)?,
        clock_domain_id: abi::copy_str(frame.header.clock_domain_id, true).map_err(invalid)?,
        stream_id: abi::copy_str(frame.header.stream_id, true).map_err(invalid)?,
        trace_id: abi::copy_str(frame.header.trace_id, true).map_err(invalid)?,
    };
    let payload = match frame.header.frame_type {
        1 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.audio };
            if value.reserved0 != 0 || value.reserved != [0; 2] {
                return Err(invalid("audio reserved fields must be zero"));
            }
            if !(1..=768_000).contains(&value.sample_rate_hz)
                || !(1..=1_024).contains(&value.channels)
                || !(1..=6).contains(&value.sample_format)
                || !(1..=2).contains(&value.layout)
                || value.samples_per_channel == 0
            {
                return Err(invalid("invalid audio shape or enum value"));
            }
            let widths = [0_u64, 1, 2, 3, 4, 4, 8];
            let width = widths[usize::from(value.sample_format)];
            let expected = value
                .samples_per_channel
                .checked_mul(u64::from(value.channels))
                .and_then(|count| count.checked_mul(width))
                .and_then(|count| usize::try_from(count).ok())
                .ok_or_else(|| invalid("audio byte-count arithmetic overflow"))?;
            if expected != value.bytes.len {
                return Err(invalid("audio byte length does not match its shape"));
            }
            OwnedPayload::Audio {
                bytes: abi::copy_bytes(value.bytes).map_err(invalid)?,
                sample_rate_hz: value.sample_rate_hz,
                channels: value.channels,
                sample_format: value.sample_format,
                layout: value.layout,
                samples_per_channel: value.samples_per_channel,
            }
        }
        2 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.video };
            if value.reserved != [0; 4] || value.width == 0 || value.height == 0 {
                return Err(invalid("invalid video payload"));
            }
            let pixels = value
                .width
                .checked_mul(value.height)
                .ok_or_else(|| invalid("video dimension arithmetic overflow"))?;
            let pixels = usize::try_from(pixels)
                .map_err(|_| invalid("video dimension arithmetic overflow"))?;
            let expected = match (value.pixel_format, value.plane_count) {
                (1, 1) => pixels
                    .checked_mul(4)
                    .ok_or_else(|| invalid("video byte-count arithmetic overflow"))?,
                (2, 3) if value.width % 2 == 0 && value.height % 2 == 0 => pixels
                    .checked_add(pixels / 2)
                    .ok_or_else(|| invalid("video byte-count arithmetic overflow"))?,
                _ => return Err(invalid("unsupported video pixel format or plane count")),
            };
            if expected != value.bytes.len {
                return Err(invalid("video byte length does not match its tight layout"));
            }
            OwnedPayload::Video {
                bytes: abi::copy_bytes(value.bytes).map_err(invalid)?,
                width: value.width,
                height: value.height,
                pixel_format: value.pixel_format,
                plane_count: value.plane_count,
            }
        }
        3 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.text };
            if value.reserved != [0; 2] {
                return Err(invalid("text reserved fields must be zero"));
            }
            let _media_type = abi::copy_utf8(value.media_type).map_err(invalid)?;
            OwnedPayload::Text(abi::copy_utf8(value.text).map_err(invalid)?)
        }
        4 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.bytes };
            if value.reserved != [0; 2] {
                return Err(invalid("byte reserved fields must be zero"));
            }
            let media_type = abi::copy_utf8(value.media_type).map_err(invalid)?;
            OwnedPayload::Byte {
                bytes: abi::copy_bytes(value.bytes).map_err(invalid)?,
                media_type: (!media_type.is_empty()).then_some(media_type),
            }
        }
        5 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.signal };
            if value.reserved != [0; 2] {
                return Err(invalid("signal reserved fields must be zero"));
            }
            let _name = abi::copy_str(value.signal_name, true).map_err(invalid)?;
            let _source = abi::copy_str(value.source_node_id, true).map_err(invalid)?;
            OwnedPayload::Signal(abi::copy_bytes(value.value).map_err(invalid)?)
        }
        6 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.event };
            if value.reserved != [0; 2] {
                return Err(invalid("event reserved fields must be zero"));
            }
            let _topic = abi::copy_str(value.topic, true).map_err(invalid)?;
            OwnedPayload::Event(abi::copy_bytes(value.value).map_err(invalid)?)
        }
        _ => return Err(invalid("unknown frame type")),
    };
    Ok(OwnedFrame { header, payload })
}

impl OwnedFrame {
    pub fn copied_payload_len(&self) -> usize {
        debug_assert_eq!(self.header.frame_type, self.payload.frame_type());
        match &self.payload {
            OwnedPayload::Audio { bytes, .. }
            | OwnedPayload::Video { bytes, .. }
            | OwnedPayload::Byte { bytes, .. }
            | OwnedPayload::Signal(bytes)
            | OwnedPayload::Event(bytes) => bytes.len(),
            OwnedPayload::Text(text) => text.len(),
        }
    }

    pub fn to_rust_text(&self) -> Result<Frame, FfiError> {
        let OwnedPayload::Text(text) = &self.payload else {
            return Err(invalid("focused graph harness accepts text frames only"));
        };
        let clock = match self.header.clock_kind {
            1 => ClockKind::Monotonic,
            2 => ClockKind::MediaRelative,
            3 => ClockKind::WallClock,
            _ => return Err(invalid("unknown clock kind")),
        };
        let header = RustHeader::new(
            FrameId::new(self.header.frame_id.clone()).map_err(|_| invalid("invalid frame ID"))?,
            Timestamp::from_nanos(self.header.timestamp_ns),
            ClockDomain::new(
                ClockDomainId::new(self.header.clock_domain_id.clone())
                    .map_err(|_| invalid("invalid clock domain ID"))?,
                clock,
            ),
            SequenceId::new(self.header.sequence_id),
            StreamId::new(self.header.stream_id.clone())
                .map_err(|_| invalid("invalid stream ID"))?,
            TraceId::new(self.header.trace_id.clone()).map_err(|_| invalid("invalid trace ID"))?,
            FrameType::Text,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )
        .map_err(|_| invalid("invalid text frame header"))?;
        Frame::new(header, RustPayload::Text(TextData::new(text.clone())))
            .map_err(|_| invalid("invalid text frame"))
    }

    pub fn into_rust(self) -> Result<Frame, FfiError> {
        let clock = match self.header.clock_kind {
            1 => ClockKind::Monotonic,
            2 => ClockKind::MediaRelative,
            3 => ClockKind::WallClock,
            _ => return Err(invalid("unknown clock kind")),
        };
        let frame_type = match self.header.frame_type {
            1 => FrameType::Audio,
            2 => FrameType::Video,
            3 => FrameType::Text,
            4 => FrameType::Byte,
            _ => {
                return Err(invalid(
                    "graph port payload must be audio, video, text, or byte",
                ))
            }
        };
        let header = RustHeader::new(
            FrameId::new(self.header.frame_id).map_err(|_| invalid("invalid frame ID"))?,
            Timestamp::from_nanos(self.header.timestamp_ns),
            ClockDomain::new(
                ClockDomainId::new(self.header.clock_domain_id)
                    .map_err(|_| invalid("invalid clock domain ID"))?,
                clock,
            ),
            SequenceId::new(self.header.sequence_id),
            StreamId::new(self.header.stream_id).map_err(|_| invalid("invalid stream ID"))?,
            TraceId::new(self.header.trace_id).map_err(|_| invalid("invalid trace ID"))?,
            frame_type,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )
        .map_err(|_| invalid("invalid frame header"))?;
        let payload = match self.payload {
            OwnedPayload::Text(text) => RustPayload::Text(TextData::new(text)),
            OwnedPayload::Byte { bytes, media_type } => RustPayload::Byte(ByteData::new(
                FrameBuffer::from_vec(bytes),
                media_type
                    .map(MediaType::new)
                    .transpose()
                    .map_err(|_| invalid("invalid media type"))?,
            )),
            OwnedPayload::Audio {
                bytes,
                sample_rate_hz,
                channels,
                sample_format,
                layout,
                samples_per_channel,
            } => {
                let format = match sample_format {
                    1 => PcmSampleFormat::U8,
                    2 => PcmSampleFormat::I16Le,
                    3 => PcmSampleFormat::I24Le,
                    4 => PcmSampleFormat::I32Le,
                    5 => PcmSampleFormat::F32Le,
                    6 => PcmSampleFormat::F64Le,
                    _ => return Err(invalid("invalid PCM format")),
                };
                let layout = match layout {
                    1 => AudioLayout::Interleaved,
                    2 => AudioLayout::Planar,
                    _ => return Err(invalid("invalid audio layout")),
                };
                RustPayload::Audio(
                    AudioData::new(
                        FrameBuffer::from_vec(bytes),
                        sample_rate_hz,
                        channels,
                        format,
                        layout,
                        samples_per_channel,
                    )
                    .map_err(|_| invalid("invalid audio frame"))?,
                )
            }
            OwnedPayload::Video {
                bytes,
                width,
                height,
                pixel_format,
                plane_count,
            } => {
                let buffer = FrameBuffer::from_vec(bytes);
                let data = match (pixel_format, plane_count) {
                    (1, 1) => VideoData::rgba8(
                        buffer,
                        width,
                        height,
                        usize::try_from(width)
                            .ok()
                            .and_then(|width| width.checked_mul(4))
                            .ok_or_else(|| invalid("invalid RGBA8 video stride"))?,
                    )
                    .map_err(|_| invalid("invalid RGBA8 video"))?,
                    (2, 3) => VideoData::yuv420p(
                        buffer,
                        width,
                        height,
                        usize::try_from(width).map_err(|_| invalid("invalid I420 width"))?,
                        usize::try_from(width / 2)
                            .map_err(|_| invalid("invalid I420 chroma width"))?,
                        usize::try_from(width / 2)
                            .map_err(|_| invalid("invalid I420 chroma width"))?,
                    )
                    .map_err(|_| invalid("invalid I420 video"))?,
                    _ => return Err(invalid("unsupported C++ graph video layout")),
                };
                RustPayload::Video(data)
            }
            OwnedPayload::Signal(_) | OwnedPayload::Event(_) => {
                return Err(invalid("control frames cannot use graph data ports"))
            }
        };
        Frame::new(header, payload).map_err(|_| invalid("invalid frame"))
    }
}

/// Owns one native payload allocation after an ABI ownership transfer.
/// Keeping the Node Pack library pinned guarantees that the release callback
/// remains mapped until the final FrameBuffer clone is dropped.
pub struct ForeignPayloadOwner {
    data: NonNull<u8>,
    len: usize,
    owner: NonNull<c_void>,
    release: abi::OwnedPayloadReleaseCallback,
    _library: Option<Arc<Library>>,
}

// SAFETY: the ABI contract makes the payload immutable after transfer and
// requires `release` to accept destruction from any Runtime worker thread.
unsafe impl Send for ForeignPayloadOwner {}

impl ForeignPayloadOwner {
    pub fn from_view(
        view: &OwnedNamedFrameView,
        library: Option<&Arc<Library>>,
    ) -> Result<Self, FfiError> {
        let owner = NonNull::new(view.payload_owner)
            .ok_or_else(|| invalid("owned emission has no payload owner"))?;
        let release = view
            .release_payload
            .ok_or_else(|| invalid("owned emission has no payload release callback"))?;
        // Establish the release guard before validating any borrowed frame
        // fields. From this point onward the host owns the native allocation,
        // including every error path.
        let mut result = Self {
            data: NonNull::dangling(),
            len: 0,
            owner,
            release,
            _library: library.cloned(),
        };
        let bytes = payload_bytes_view(&view.frame)?;
        let data = if bytes.len == 0 {
            NonNull::dangling()
        } else {
            NonNull::new(bytes.data.cast_mut())
                .ok_or_else(|| invalid("owned emission has a null payload"))?
        };
        result.data = data;
        result.len = bytes.len;
        if view.reserved != [0; 2] {
            return Err(invalid("owned emission reserved fields must be zero"));
        }
        if result.len > abi::MAX_COPY_BYTES {
            return Err(invalid("owned emission exceeds the maximum Frame payload"));
        }
        Ok(result)
    }
}

impl AsRef<[u8]> for ForeignPayloadOwner {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: the native owner guarantees an immutable allocation of `len`
        // bytes until `release` is called from Drop. Bytes calls AsRef once and
        // retains this owner for the complete slice lifetime.
        unsafe { std::slice::from_raw_parts(self.data.as_ptr(), self.len) }
    }
}

impl Drop for ForeignPayloadOwner {
    fn drop(&mut self) {
        (self.release)(self.owner.as_ptr());
    }
}

pub fn adopt_owned_frame(
    pointer: *const FrameView,
    owner: ForeignPayloadOwner,
) -> Result<Frame, FfiError> {
    if !abi::aligned(pointer) {
        return Err(invalid("owned frame pointer is null or unaligned"));
    }
    let frame = abi::read_copy(pointer).ok_or_else(|| invalid("owned frame is not readable"))?;
    let expected = u32::try_from(mem::size_of::<abi::FrameHeader>()).unwrap_or(u32::MAX);
    if frame.header.abi_version != abi::ABI_VERSION || frame.header.struct_size != expected {
        return Err(FfiError::abi(
            "owned frame header version or size does not match v1",
        ));
    }
    if frame.header.reserved != [0; 4] {
        return Err(invalid("owned frame header reserved fields must be zero"));
    }
    let clock = match frame.header.clock_kind {
        1 => ClockKind::Monotonic,
        2 => ClockKind::MediaRelative,
        3 => ClockKind::WallClock,
        _ => return Err(invalid("unknown clock kind")),
    };
    let frame_type = match frame.header.frame_type {
        1 => FrameType::Audio,
        2 => FrameType::Video,
        4 => FrameType::Byte,
        _ => return Err(invalid("owned emission must be Audio, Video, or Byte")),
    };
    let header = RustHeader::new(
        FrameId::new(abi::copy_str(frame.header.frame_id, true).map_err(invalid)?)
            .map_err(|_| invalid("invalid frame ID"))?,
        Timestamp::from_nanos(frame.header.timestamp_ns),
        ClockDomain::new(
            ClockDomainId::new(abi::copy_str(frame.header.clock_domain_id, true).map_err(invalid)?)
                .map_err(|_| invalid("invalid clock domain ID"))?,
            clock,
        ),
        SequenceId::new(frame.header.sequence_id),
        StreamId::new(abi::copy_str(frame.header.stream_id, true).map_err(invalid)?)
            .map_err(|_| invalid("invalid stream ID"))?,
        TraceId::new(abi::copy_str(frame.header.trace_id, true).map_err(invalid)?)
            .map_err(|_| invalid("invalid trace ID"))?,
        frame_type,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .map_err(|_| invalid("invalid owned frame header"))?;

    let buffer = FrameBuffer::from_owner(owner);
    let payload = match frame.header.frame_type {
        1 => {
            // SAFETY: frame_type selects the initialized audio union member.
            let value = unsafe { frame.payload.audio };
            validate_audio_payload(&value, buffer.len())?;
            RustPayload::Audio(
                AudioData::new(
                    buffer,
                    value.sample_rate_hz,
                    value.channels,
                    pcm_sample_format(value.sample_format)?,
                    audio_layout(value.layout)?,
                    value.samples_per_channel,
                )
                .map_err(|_| invalid("invalid owned audio frame"))?,
            )
        }
        2 => {
            // SAFETY: frame_type selects the initialized video union member.
            let value = unsafe { frame.payload.video };
            validate_video_payload(&value, buffer.len())?;
            RustPayload::Video(match (value.pixel_format, value.plane_count) {
                (1, 1) => VideoData::rgba8(
                    buffer,
                    value.width,
                    value.height,
                    usize::try_from(value.width)
                        .ok()
                        .and_then(|width| width.checked_mul(4))
                        .ok_or_else(|| invalid("invalid RGBA8 video stride"))?,
                )
                .map_err(|_| invalid("invalid owned RGBA8 video"))?,
                (2, 3) => VideoData::yuv420p(
                    buffer,
                    value.width,
                    value.height,
                    usize::try_from(value.width).map_err(|_| invalid("invalid I420 width"))?,
                    usize::try_from(value.width / 2)
                        .map_err(|_| invalid("invalid I420 chroma width"))?,
                    usize::try_from(value.width / 2)
                        .map_err(|_| invalid("invalid I420 chroma width"))?,
                )
                .map_err(|_| invalid("invalid owned I420 video"))?,
                _ => return Err(invalid("unsupported owned video layout")),
            })
        }
        4 => {
            // SAFETY: frame_type selects the initialized byte union member.
            let value = unsafe { frame.payload.bytes };
            if value.reserved != [0; 2] {
                return Err(invalid("byte reserved fields must be zero"));
            }
            let media_type = abi::copy_utf8(value.media_type).map_err(invalid)?;
            RustPayload::Byte(ByteData::new(
                buffer,
                (!media_type.is_empty())
                    .then(|| MediaType::new(media_type))
                    .transpose()
                    .map_err(|_| invalid("invalid media type"))?,
            ))
        }
        _ => unreachable!("validated owned frame type"),
    };
    Frame::new(header, payload).map_err(|_| invalid("invalid owned frame"))
}

fn payload_bytes_view(frame: &FrameView) -> Result<abi::BytesView, FfiError> {
    match frame.header.frame_type {
        // SAFETY: the frame type selects the initialized union member.
        1 => Ok(unsafe { frame.payload.audio }.bytes),
        2 => Ok(unsafe { frame.payload.video }.bytes),
        4 => Ok(unsafe { frame.payload.bytes }.bytes),
        _ => Err(invalid("owned emission must be Audio, Video, or Byte")),
    }
}

fn validate_audio_payload(value: &abi::AudioPayload, actual_len: usize) -> Result<(), FfiError> {
    if value.reserved0 != 0 || value.reserved != [0; 2] {
        return Err(invalid("audio reserved fields must be zero"));
    }
    if !(1..=768_000).contains(&value.sample_rate_hz)
        || !(1..=1_024).contains(&value.channels)
        || value.samples_per_channel == 0
    {
        return Err(invalid("invalid audio shape"));
    }
    let widths = [0_u64, 1, 2, 3, 4, 4, 8];
    let width = *widths
        .get(usize::from(value.sample_format))
        .filter(|width| **width != 0)
        .ok_or_else(|| invalid("invalid PCM format"))?;
    let expected = value
        .samples_per_channel
        .checked_mul(u64::from(value.channels))
        .and_then(|count| count.checked_mul(width))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| invalid("audio byte-count arithmetic overflow"))?;
    if expected != actual_len || !(1..=2).contains(&value.layout) {
        return Err(invalid("audio byte length or layout is invalid"));
    }
    Ok(())
}

fn validate_video_payload(value: &abi::VideoPayload, actual_len: usize) -> Result<(), FfiError> {
    if value.reserved != [0; 4] || value.width == 0 || value.height == 0 {
        return Err(invalid("invalid video payload"));
    }
    let pixels = usize::try_from(
        value
            .width
            .checked_mul(value.height)
            .ok_or_else(|| invalid("video dimension arithmetic overflow"))?,
    )
    .map_err(|_| invalid("video dimension arithmetic overflow"))?;
    let expected =
        match (value.pixel_format, value.plane_count) {
            (1, 1) => pixels
                .checked_mul(4)
                .ok_or_else(|| invalid("video byte-count arithmetic overflow"))?,
            (2, 3) if value.width % 2 == 0 && value.height % 2 == 0 => pixels
                .checked_add(pixels / 2)
                .ok_or_else(|| invalid("video byte-count arithmetic overflow"))?,
            _ => return Err(invalid("unsupported video pixel format or plane count")),
        };
    if expected != actual_len {
        return Err(invalid("video byte length does not match its layout"));
    }
    Ok(())
}

fn pcm_sample_format(value: u16) -> Result<PcmSampleFormat, FfiError> {
    match value {
        1 => Ok(PcmSampleFormat::U8),
        2 => Ok(PcmSampleFormat::I16Le),
        3 => Ok(PcmSampleFormat::I24Le),
        4 => Ok(PcmSampleFormat::I32Le),
        5 => Ok(PcmSampleFormat::F32Le),
        6 => Ok(PcmSampleFormat::F64Le),
        _ => Err(invalid("invalid PCM format")),
    }
}

fn audio_layout(value: u32) -> Result<AudioLayout, FfiError> {
    match value {
        1 => Ok(AudioLayout::Interleaved),
        2 => Ok(AudioLayout::Planar),
        _ => Err(invalid("invalid audio layout")),
    }
}

impl OwnedPayload {
    fn frame_type(&self) -> u32 {
        match self {
            Self::Audio { .. } => 1,
            Self::Video { .. } => 2,
            Self::Text(_) => 3,
            Self::Byte { .. } => 4,
            Self::Signal(_) => 5,
            Self::Event(_) => 6,
        }
    }
}

pub fn borrowed_text_view(frame: &Frame) -> Result<FrameView, FfiError> {
    let Some(text) = frame.as_text() else {
        return Err(invalid("foreign text node received a non-text frame"));
    };
    let header = frame.header();
    let mut view = abi::empty_frame_view();
    view.header.frame_type = 3;
    view.header.clock_kind = match header.clock_domain().kind() {
        ClockKind::Monotonic => 1,
        ClockKind::MediaRelative => 2,
        ClockKind::WallClock => 3,
    };
    view.header.timestamp_ns = header.timestamp().as_nanos();
    view.header.sequence_id = header.sequence_id().get();
    view.header.frame_id = str_view(header.frame_id().as_str());
    view.header.clock_domain_id = str_view(header.clock_domain().id().as_str());
    view.header.stream_id = str_view(header.stream_id().as_str());
    view.header.trace_id = str_view(header.trace_id().as_str());
    view.payload = FramePayload {
        text: abi::TextPayload {
            text: str_view(text.data().as_str()),
            media_type: StrView {
                data: std::ptr::null(),
                len: 0,
            },
            reserved: [0; 2],
        },
    };
    Ok(view)
}

pub fn borrowed_frame_view(frame: &Frame) -> Result<FrameView, FfiError> {
    if frame.as_text().is_some() {
        return borrowed_text_view(frame);
    }
    let header = frame.header();
    let mut view = abi::empty_frame_view();
    view.header.frame_type = match frame.frame_type() {
        FrameType::Audio => 1,
        FrameType::Video => 2,
        FrameType::Text => 3,
        FrameType::Byte => 4,
        FrameType::Signal => 5,
        FrameType::Event => 6,
    };
    view.header.clock_kind = match header.clock_domain().kind() {
        ClockKind::Monotonic => 1,
        ClockKind::MediaRelative => 2,
        ClockKind::WallClock => 3,
    };
    view.header.timestamp_ns = header.timestamp().as_nanos();
    view.header.sequence_id = header.sequence_id().get();
    view.header.frame_id = str_view(header.frame_id().as_str());
    view.header.clock_domain_id = str_view(header.clock_domain().id().as_str());
    view.header.stream_id = str_view(header.stream_id().as_str());
    view.header.trace_id = str_view(header.trace_id().as_str());
    view.payload = match frame {
        Frame::Audio(frame) => {
            let data = frame.data();
            abi::FramePayload {
                audio: abi::AudioPayload {
                    sample_rate_hz: data.sample_rate_hz(),
                    channels: data.channels(),
                    sample_format: match data.sample_format() {
                        PcmSampleFormat::U8 => 1,
                        PcmSampleFormat::I16Le => 2,
                        PcmSampleFormat::I24Le => 3,
                        PcmSampleFormat::I32Le => 4,
                        PcmSampleFormat::F32Le => 5,
                        PcmSampleFormat::F64Le => 6,
                    },
                    layout: if data.layout() == AudioLayout::Planar {
                        2
                    } else {
                        1
                    },
                    reserved0: 0,
                    samples_per_channel: data.samples_per_channel(),
                    bytes: abi::BytesView {
                        data: data.buffer().as_slice().as_ptr(),
                        len: data.buffer().len(),
                    },
                    reserved: [0; 2],
                },
            }
        }
        Frame::Video(frame) => {
            let data = frame.data();
            let (pixel_format, plane_count) = match (data.pixel_format(), data.layout()) {
                (PixelFormat::Rgba8, VideoLayout::Rgba8 { .. }) => (1, 1),
                (PixelFormat::Yuv420p, VideoLayout::Yuv420p { .. }) => (2, 3),
                _ => return Err(invalid("video pixel format and layout disagree")),
            };
            abi::FramePayload {
                video: abi::VideoPayload {
                    width: data.width(),
                    height: data.height(),
                    pixel_format,
                    plane_count,
                    bytes: abi::BytesView {
                        data: data.buffer().as_slice().as_ptr(),
                        len: data.buffer().len(),
                    },
                    reserved: [0; 4],
                },
            }
        }
        Frame::Byte(frame) => {
            let data = frame.data();
            abi::FramePayload {
                bytes: abi::BytePayload {
                    bytes: abi::BytesView {
                        data: data.buffer().as_slice().as_ptr(),
                        len: data.buffer().len(),
                    },
                    media_type: data.media_type().map_or(
                        StrView {
                            data: std::ptr::null(),
                            len: 0,
                        },
                        |value| str_view(value.as_str()),
                    ),
                    reserved: [0; 2],
                },
            }
        }
        Frame::Signal(frame) => abi::FramePayload {
            signal: abi::SignalPayload {
                signal_name: str_view(frame.data().name().as_str()),
                source_node_id: str_view(frame.data().source().as_str()),
                // ABI v1 leaves structured Value serialization implementation-defined.
                value: abi::BytesView {
                    data: std::ptr::null(),
                    len: 0,
                },
                reserved: [0; 2],
            },
        },
        Frame::Event(frame) => abi::FramePayload {
            event: abi::EventPayload {
                topic: str_view(frame.data().topic().as_str()),
                value: abi::BytesView {
                    data: std::ptr::null(),
                    len: 0,
                },
                reserved: [0; 2],
            },
        },
        Frame::Text(_) => unreachable!(),
    };
    Ok(view)
}

fn str_view(value: &str) -> StrView {
    StrView {
        data: value.as_ptr().cast(),
        len: value.len(),
    }
}

fn invalid(message: &'static str) -> FfiError {
    FfiError::validation("MUXIVA-FFI-FRAME", message)
}
