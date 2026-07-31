use std::mem;

use voxa_types::{
    ClockDomain, ClockDomainId, ClockKind, Extensions, Frame, FrameHeader as RustHeader, FrameId,
    FramePayload as RustPayload, FrameType, Lineage, Metadata, SequenceId, StreamId, TextData,
    Timestamp, TraceId,
};

use crate::{
    abi::{self, FramePayload, FrameView, StrView},
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
    Audio(Vec<u8>),
    Video(Vec<u8>),
    Text(String),
    Byte(Vec<u8>),
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
            OwnedPayload::Audio(abi::copy_bytes(value.bytes).map_err(invalid)?)
        }
        2 => {
            // SAFETY: the discriminating frame_type selects the initialized C union member.
            let value = unsafe { frame.payload.video };
            if value.reserved != [0; 4]
                || value.width == 0
                || value.height == 0
                || !(1..=8).contains(&value.pixel_format)
                || !(1..=4).contains(&value.plane_count)
            {
                return Err(invalid("invalid video payload"));
            }
            let _pixels = value
                .width
                .checked_mul(value.height)
                .ok_or_else(|| invalid("video dimension arithmetic overflow"))?;
            OwnedPayload::Video(abi::copy_bytes(value.bytes).map_err(invalid)?)
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
            let _media_type = abi::copy_utf8(value.media_type).map_err(invalid)?;
            OwnedPayload::Byte(abi::copy_bytes(value.bytes).map_err(invalid)?)
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
            OwnedPayload::Audio(bytes)
            | OwnedPayload::Video(bytes)
            | OwnedPayload::Byte(bytes)
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
}

impl OwnedPayload {
    fn frame_type(&self) -> u32 {
        match self {
            Self::Audio(_) => 1,
            Self::Video(_) => 2,
            Self::Text(_) => 3,
            Self::Byte(_) => 4,
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

fn str_view(value: &str) -> StrView {
    StrView {
        data: value.as_ptr().cast(),
        len: value.len(),
    }
}

fn invalid(message: &'static str) -> FfiError {
    FfiError::validation("VOXA-FFI-FRAME", message)
}
