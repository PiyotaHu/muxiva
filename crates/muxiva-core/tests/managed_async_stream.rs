use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use muxiva_core::{
    AdapterResponse, AdmissionSlots, AsyncRequest, DeliveryOrdering, ManagedAsyncStream,
    ManagedStreamAdapter, ManagedStreamOptions, RequestId, ServiceError, StreamResult,
    SubmitOutcome,
};
use muxiva_types::{
    ClockDomain, ClockDomainId, ClockKind, Extensions, Frame, FrameHeader, FrameId, FramePayload,
    FrameType, Lineage, Metadata, SequenceId, SessionId, StreamId, TextData, Timestamp, TraceId,
};

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let open = self.open.lock().unwrap();
        drop(self.changed.wait_while(open, |open| !*open).unwrap());
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

fn text_frame(sequence: u64) -> Frame {
    let header = FrameHeader::new(
        FrameId::new(format!("managed-{sequence}")).unwrap(),
        Timestamp::from_nanos(sequence as i64),
        ClockDomain::new(
            ClockDomainId::new("managed-clock").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(sequence),
        StreamId::new("managed-input").unwrap(),
        TraceId::new("managed-trace").unwrap(),
        FrameType::Text,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(
        header,
        FramePayload::Text(TextData::new(sequence.to_string())),
    )
    .unwrap()
}

fn options(
    input_capacity: usize,
    result_capacity: usize,
    in_flight: usize,
) -> ManagedStreamOptions {
    ManagedStreamOptions {
        input_capacity: NonZeroUsize::new(input_capacity).unwrap(),
        result_capacity: NonZeroUsize::new(result_capacity).unwrap(),
        max_in_flight: NonZeroUsize::new(in_flight).unwrap(),
        max_response_frames: NonZeroUsize::new(64).unwrap(),
        max_response_bytes: NonZeroUsize::new(1024 * 1024).unwrap(),
        ordering: DeliveryOrdering::Strict,
        reconnect_delay: Duration::ZERO,
        thread_name: "muxiva-managed-test".into(),
    }
}

fn request(
    request_id: u64,
    session: &SessionId,
    input: Frame,
    deadline: Instant,
    attempts: usize,
    admission: &AdmissionSlots,
) -> AsyncRequest {
    AsyncRequest {
        request_id: RequestId::new(request_id),
        session_id: session.clone(),
        input,
        deadline,
        attempt_limit: NonZeroUsize::new(attempts).unwrap(),
        admission: admission.try_acquire().unwrap().unwrap(),
    }
}

fn stop(stream: &ManagedAsyncStream) {
    assert!(stream.stop(Duration::from_secs(1)).executor_finished);
}

struct RetryAdapter {
    sends: Arc<AtomicUsize>,
    reconnect_connects: Arc<AtomicUsize>,
}

impl ManagedStreamAdapter for RetryAdapter {
    fn connect(&self, _session_id: &SessionId, reconnecting: bool) -> Result<(), ServiceError> {
        if reconnecting {
            self.reconnect_connects.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn send(&self, request: muxiva_core::AdapterRequest) -> AdapterResponse {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if request.request_id == RequestId::new(5) {
            panic!("simulated adapter panic");
        }
        if request.request_id == RequestId::new(4) {
            return AdapterResponse::Failed(ServiceError::new("fatal", "service rejected input"));
        }
        if request.attempt == 1 {
            AdapterResponse::Retryable(ServiceError::new("half_open", "probe rejected"))
        } else {
            AdapterResponse::Frames(vec![request.input])
        }
    }
}

#[test]
fn network_wait_is_off_caller_thread_and_one_session_cannot_block_another_or_bypass_work() {
    let slow_session = SessionId::new("slow-session").unwrap();
    let fast_session = SessionId::new("fast-session").unwrap();
    let slow_gate = Arc::new(Gate::default());
    let adapter_gate = slow_gate.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let slow = ManagedAsyncStream::new(
        slow_session.clone(),
        options(2, 2, 1),
        move |request: muxiva_core::AdapterRequest| {
            started_tx.send(thread::current().id()).unwrap();
            adapter_gate.wait();
            AdapterResponse::Frames(vec![request.input])
        },
    )
    .unwrap();
    let fast = ManagedAsyncStream::new(
        fast_session.clone(),
        options(2, 2, 1),
        |request: muxiva_core::AdapterRequest| AdapterResponse::Frames(vec![request.input]),
    )
    .unwrap();
    let admissions = AdmissionSlots::new(2).unwrap();
    let caller = thread::current().id();
    assert_eq!(
        slow.try_submit(request(
            1,
            &slow_session,
            text_frame(1),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert_ne!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        caller
    );

    // Ordinary caller-owned/bypass work remains immediately runnable.
    let bypass = AtomicUsize::new(0);
    bypass.fetch_add(1, Ordering::SeqCst);
    assert_eq!(bypass.load(Ordering::SeqCst), 1);

    assert_eq!(
        fast.try_submit(request(
            2,
            &fast_session,
            text_frame(2),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    let fast_result = fast.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(fast_result.request_id, RequestId::new(2));
    assert!(slow.try_recv().is_none());

    slow_gate.open();
    assert_eq!(
        slow.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .request_id,
        RequestId::new(1)
    );
    stop(&slow);
    stop(&fast);
}

#[test]
fn independent_in_flight_workers_complete_out_of_order_but_strict_results_do_not() {
    let session = SessionId::new("ordered-session").unwrap();
    let first_gate = Arc::new(Gate::default());
    let second_gate = Arc::new(Gate::default());
    let first_adapter_gate = first_gate.clone();
    let second_adapter_gate = second_gate.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (responded_tx, responded_rx) = mpsc::channel();
    let stream = ManagedAsyncStream::new(
        session.clone(),
        options(2, 2, 2),
        move |request: muxiva_core::AdapterRequest| {
            started_tx.send(request.request_id).unwrap();
            if request.request_id == RequestId::new(40) {
                first_adapter_gate.wait();
            } else {
                second_adapter_gate.wait();
            }
            responded_tx.send(request.request_id).unwrap();
            AdapterResponse::Frames(vec![request.input])
        },
    )
    .unwrap();
    let admissions = AdmissionSlots::new(2).unwrap();
    for id in [40, 10] {
        assert_eq!(
            stream.try_submit(request(
                id,
                &session,
                text_frame(id),
                Instant::now() + Duration::from_secs(2),
                1,
                &admissions,
            )),
            SubmitOutcome::Accepted
        );
    }
    let started = [
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
    ];
    assert!(started.contains(&RequestId::new(40)) && started.contains(&RequestId::new(10)));
    assert_eq!(stream.metrics().peak_active_requests, 2);

    second_gate.open();
    assert_eq!(
        responded_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        RequestId::new(10)
    );
    assert!(stream.recv_timeout(Duration::from_millis(25)).is_none());
    first_gate.open();
    assert_eq!(
        responded_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        RequestId::new(40)
    );
    assert_eq!(
        stream
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .request_id,
        RequestId::new(40)
    );
    assert_eq!(
        stream
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .request_id,
        RequestId::new(10)
    );
    stop(&stream);
}

#[test]
fn timeout_and_retry_boundaries_are_terminal_and_reconnect_is_counted() {
    let session = SessionId::new("retry-session").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let reconnect_connects = Arc::new(AtomicUsize::new(0));
    let stream = ManagedAsyncStream::new(
        session.clone(),
        options(4, 4, 1),
        RetryAdapter {
            sends: calls.clone(),
            reconnect_connects: reconnect_connects.clone(),
        },
    )
    .unwrap();
    let admissions = AdmissionSlots::new(3).unwrap();
    assert_eq!(
        stream.try_submit(request(
            1,
            &session,
            text_frame(1),
            Instant::now() + Duration::from_secs(1),
            2,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert!(matches!(
        stream.recv_timeout(Duration::from_secs(1)).unwrap().result,
        StreamResult::Frames(_)
    ));
    let retry_metrics = stream.metrics();
    assert_eq!(retry_metrics.attempts, 2);
    assert_eq!(retry_metrics.retries, 1);
    assert_eq!(retry_metrics.reconnects, 1);
    assert_eq!(reconnect_connects.load(Ordering::SeqCst), 1);

    let before_expired = calls.load(Ordering::SeqCst);
    assert_eq!(
        stream.try_submit(request(
            2,
            &session,
            text_frame(2),
            Instant::now(),
            3,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    let timeout = stream.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        timeout.result,
        StreamResult::Failed(ref error) if error.code() == "managed_stream_timeout"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), before_expired);

    assert_eq!(
        stream.try_submit(request(
            3,
            &session,
            text_frame(3),
            Instant::now() + Duration::from_secs(1),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert!(matches!(
        stream.recv_timeout(Duration::from_secs(1)).unwrap().result,
        StreamResult::Retryable(ref error) if error.code() == "half_open"
    ));
    assert_eq!(stream.metrics().retry_exhausted, 1);

    assert_eq!(
        stream.try_submit(request(
            4,
            &session,
            text_frame(4),
            Instant::now() + Duration::from_secs(1),
            3,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert!(matches!(
        stream.recv_timeout(Duration::from_secs(1)).unwrap().result,
        StreamResult::Failed(ref error) if error.code() == "fatal"
    ));
    assert_eq!(stream.metrics().failed, 1);

    assert_eq!(
        stream.try_submit(request(
            5,
            &session,
            text_frame(5),
            Instant::now() + Duration::from_secs(1),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert!(matches!(
        stream.recv_timeout(Duration::from_secs(1)).unwrap().result,
        StreamResult::Failed(ref error) if error.code() == "managed_stream_adapter_panic"
    ));
    assert_eq!(stream.metrics().failed, 2);
    stop(&stream);
}

#[test]
fn active_deadline_releases_admission_before_a_late_service_response() {
    let session = SessionId::new("deadline-session").unwrap();
    let gate = Arc::new(Gate::default());
    let adapter_gate = gate.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let stream = ManagedAsyncStream::new(
        session.clone(),
        options(1, 1, 1),
        move |request: muxiva_core::AdapterRequest| {
            started_tx.send(()).unwrap();
            adapter_gate.wait();
            AdapterResponse::Frames(vec![request.input])
        },
    )
    .unwrap();
    let admissions = AdmissionSlots::new(1).unwrap();
    assert_eq!(
        stream.try_submit(request(
            1,
            &session,
            text_frame(1),
            Instant::now() + Duration::from_millis(50),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let completion = stream.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        completion.result,
        StreamResult::Failed(ref error) if error.code() == "managed_stream_timeout"
    ));
    assert_eq!(admissions.snapshot().in_flight, 0);
    gate.open();
    stop(&stream);
}

#[test]
fn cancel_releases_admission_discards_late_result_and_input_mailbox_is_bounded() {
    let session = SessionId::new("bounded-session").unwrap();
    let gate = Arc::new(Gate::default());
    let adapter_gate = gate.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let stream = ManagedAsyncStream::new(
        session.clone(),
        options(1, 2, 1),
        move |request: muxiva_core::AdapterRequest| {
            started_tx.send(request.request_id).unwrap();
            if request.request_id == RequestId::new(1) {
                adapter_gate.wait();
            }
            AdapterResponse::Frames(vec![request.input])
        },
    )
    .unwrap();
    let admissions = AdmissionSlots::new(3).unwrap();
    assert_eq!(
        stream.try_submit(request(
            1,
            &session,
            text_frame(1),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        RequestId::new(1)
    );
    assert_eq!(
        stream.try_submit(request(
            2,
            &session,
            text_frame(2),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert_eq!(
        stream.try_submit(request(
            3,
            &session,
            text_frame(3),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::MailboxFull
    );
    assert_eq!(admissions.snapshot().in_flight, 2);
    assert!(stream.cancel(RequestId::new(1)));
    assert_eq!(admissions.snapshot().in_flight, 1);
    gate.open();
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        RequestId::new(2)
    );
    assert_eq!(
        stream
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .request_id,
        RequestId::new(2)
    );
    assert_eq!(admissions.snapshot().in_flight, 0);
    assert!(stream.try_recv().is_none());
    let metrics = stream.metrics();
    assert_eq!(metrics.mailbox_full, 1);
    assert_eq!(metrics.cancelled, 1);
    assert!(metrics.late_results_discarded >= 1);
    stop(&stream);
}

#[test]
fn stop_is_idempotent_rejects_new_work_and_discards_queued_results() {
    let session = SessionId::new("stopped-session").unwrap();
    let stream = ManagedAsyncStream::new(
        session.clone(),
        options(1, 1, 1),
        |request: muxiva_core::AdapterRequest| AdapterResponse::Frames(vec![request.input]),
    )
    .unwrap();
    let admissions = AdmissionSlots::new(2).unwrap();
    assert_eq!(
        stream.try_submit(request(
            1,
            &session,
            text_frame(1),
            Instant::now() + Duration::from_secs(1),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    let _ = stream.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(stream.stop(Duration::from_secs(1)).executor_finished);
    assert!(stream.stop(Duration::from_secs(1)).executor_finished);
    assert_eq!(
        stream.try_submit(request(
            2,
            &session,
            text_frame(2),
            Instant::now() + Duration::from_secs(1),
            1,
            &admissions,
        )),
        SubmitOutcome::Stopping
    );
    assert_eq!(admissions.snapshot().in_flight, 0);
}

#[test]
fn cancelled_expired_queued_request_is_a_strict_order_tombstone_not_a_timeout() {
    let session = SessionId::new("queued-cancel-race").unwrap();
    let gate = Arc::new(Gate::default());
    let adapter_gate = gate.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let stream = ManagedAsyncStream::new(
        session.clone(),
        options(3, 3, 1),
        move |request: muxiva_core::AdapterRequest| {
            started_tx.send(request.request_id).unwrap();
            if request.request_id == RequestId::new(1) {
                adapter_gate.wait();
            }
            AdapterResponse::Frames(vec![request.input])
        },
    )
    .unwrap();
    let admissions = AdmissionSlots::new(3).unwrap();
    assert_eq!(
        stream.try_submit(request(
            1,
            &session,
            text_frame(1),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        RequestId::new(1)
    );
    assert_eq!(
        stream.try_submit(request(
            2,
            &session,
            text_frame(2),
            Instant::now(),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert_eq!(
        stream.try_submit(request(
            3,
            &session,
            text_frame(3),
            Instant::now() + Duration::from_secs(2),
            1,
            &admissions,
        )),
        SubmitOutcome::Accepted
    );
    assert!(stream.cancel(RequestId::new(2)));
    gate.open();

    assert_eq!(
        stream
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .request_id,
        RequestId::new(1)
    );
    assert_eq!(
        stream
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .request_id,
        RequestId::new(3)
    );
    assert!(stream.try_recv().is_none());
    let metrics = stream.metrics();
    assert_eq!(metrics.cancelled, 1);
    assert_eq!(metrics.timed_out, 0);
    assert_eq!(metrics.completed, 3);
    stop(&stream);
}

#[test]
fn oversized_frame_count_and_bytes_become_bounded_terminal_errors() {
    let session = SessionId::new("response-limits").unwrap();
    let mut limited = options(2, 2, 1);
    limited.max_response_frames = NonZeroUsize::new(1).unwrap();
    limited.max_response_bytes = NonZeroUsize::new(3).unwrap();
    let stream = ManagedAsyncStream::new(
        session.clone(),
        limited,
        move |request: muxiva_core::AdapterRequest| match request.request_id {
            id if id == RequestId::new(1) => {
                AdapterResponse::Frames(vec![request.input.clone(), request.input])
            }
            _ => AdapterResponse::Frames(vec![request.input]),
        },
    )
    .unwrap();
    let admissions = AdmissionSlots::new(2).unwrap();
    for id in [1, 2] {
        assert_eq!(
            stream.try_submit(request(
                id,
                &session,
                text_frame(if id == 1 { 1 } else { 2_000 }),
                Instant::now() + Duration::from_secs(1),
                1,
                &admissions,
            )),
            SubmitOutcome::Accepted
        );
    }
    for id in [1, 2] {
        let completion = stream.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(completion.request_id, RequestId::new(id));
        assert!(matches!(
            completion.result,
            StreamResult::Failed(ref error)
                if error.code() == "managed_stream_response_limit"
        ));
    }
    let metrics = stream.metrics();
    assert_eq!(metrics.response_limit_exceeded, 2);
    assert_eq!(metrics.failed, 2);
    assert_eq!(metrics.succeeded, 0);
    assert_eq!(metrics.queued_results, 0);
    assert_eq!(admissions.snapshot().in_flight, 0);
    stop(&stream);
}
