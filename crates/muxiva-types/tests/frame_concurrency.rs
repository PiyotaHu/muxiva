use std::sync::{Arc, Barrier};

use muxiva_types::{
    ByteData, ClockDomain, ClockDomainId, ClockKind, Extensions, Frame, FrameBuffer, FrameHeader,
    FrameId, FramePayload, FrameType, Lineage, Metadata, SequenceId, StreamId, Timestamp, TraceId,
};

fn byte_frame() -> Frame {
    let header = FrameHeader::new(
        FrameId::new("concurrent-byte-frame").unwrap(),
        Timestamp::from_nanos(42),
        ClockDomain::new(
            ClockDomainId::new("capture.bytes").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(7),
        StreamId::new("concurrent-stream").unwrap(),
        TraceId::new("concurrent-trace").unwrap(),
        FrameType::Byte,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();

    Frame::new(
        header,
        FramePayload::Byte(ByteData::new(FrameBuffer::from_vec(vec![1, 2, 3]), None)),
    )
    .unwrap()
}

fn move_through_consumer(frame: Frame) -> Frame {
    frame
}

#[test]
fn frame_moves_and_clones_without_copying_its_byte_buffer() {
    let moved = move_through_consumer(byte_frame());
    let clone = moved.clone();

    let moved_buffer = moved.as_byte().unwrap().data().buffer();
    let cloned_buffer = clone.as_byte().unwrap().data().buffer();
    assert_eq!(moved_buffer.as_slice(), &[1, 2, 3]);
    assert_eq!(
        moved_buffer.as_slice().as_ptr(),
        cloned_buffer.as_slice().as_ptr()
    );
}

#[test]
fn frames_and_buffers_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<Frame>();
    assert_send_sync::<FrameBuffer>();
}

#[test]
fn shared_frame_is_read_concurrently_without_locks() {
    let frame = Arc::new(byte_frame());
    let barrier = Arc::new(Barrier::new(9));

    std::thread::scope(|scope| {
        for _ in 0..8 {
            let shared = Arc::clone(&frame);
            let start = Arc::clone(&barrier);
            scope.spawn(move || {
                start.wait();
                for _ in 0..1_000 {
                    assert_eq!(shared.header().frame_id().as_str(), "concurrent-byte-frame");
                    assert_eq!(shared.header().timestamp(), Timestamp::from_nanos(42));
                    assert_eq!(shared.header().sequence_id(), SequenceId::new(7));
                    assert_eq!(
                        shared.as_byte().unwrap().data().buffer().as_slice(),
                        &[1, 2, 3]
                    );
                }
            });
        }
        barrier.wait();
    });
}
