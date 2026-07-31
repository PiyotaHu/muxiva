#![no_main]

use libfuzzer_sys::fuzz_target;
use voxa_types::{FiniteF64, FrameBuffer, Value, ValueMap};

fuzz_target!(|data: &[u8]| {
    let number = data
        .get(..8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(f64::from_le_bytes)
        .unwrap_or(0.0);
    let candidate = String::from_utf8_lossy(data);
    let values = Value::List(
        vec![
            Value::Bytes(FrameBuffer::from_vec(data.to_vec())),
            Value::String(candidate.clone().into_owned().into_boxed_str()),
            FiniteF64::new(number)
                .map(Value::Float)
                .unwrap_or(Value::Null),
        ]
        .into_boxed_slice(),
    );
    let _ = ValueMap::try_from_iter([(candidate.as_ref(), values)]);
});
