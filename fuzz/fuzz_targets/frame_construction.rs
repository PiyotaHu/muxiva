#![no_main]

use libfuzzer_sys::fuzz_target;
use muxiva_types::{Extensions, FrameBuffer, FrameId, Metadata, StreamId, TextData, TraceId, Value};

fuzz_target!(|data: &[u8]| {
    let text = FrameBuffer::from_vec(data.to_vec());
    let _ = TextData::from_utf8(text);
    let candidate = String::from_utf8_lossy(data);
    let _ = FrameId::new(candidate.as_ref());
    let _ = StreamId::new(candidate.as_ref());
    let _ = TraceId::new(candidate.as_ref());
    let _ = Metadata::try_from_iter([(
        candidate.as_ref(),
        Value::Bytes(FrameBuffer::from_vec(data.to_vec())),
    )]);
    let _ = Extensions::empty();
});
