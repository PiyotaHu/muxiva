use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use muxiva_core::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigMap, ForeignCommand,
    ForeignCommandKind, ForeignCompletion, ForeignCompletionOutcome, ForeignDriverConfig,
    ForeignFullPolicy, ForeignNodeDriver, ForeignOrdering, ForeignSubmitOutcome,
};
use muxiva_types::{
    ClockDomain, ClockDomainId, ClockKind, Extensions, Frame, FrameHeader, FrameId, FramePayload,
    FrameType, Lineage, Metadata, SequenceId, StreamId, TextData, Timestamp, TraceId,
};

fn config(ordering: ForeignOrdering) -> ForeignDriverConfig {
    ForeignDriverConfig {
        command_capacity: NonZeroUsize::new(2).unwrap(),
        command_byte_capacity: NonZeroUsize::new(1024).unwrap(),
        completion_capacity: NonZeroUsize::new(2).unwrap(),
        completion_byte_capacity: NonZeroUsize::new(1024).unwrap(),
        max_in_flight: NonZeroUsize::new(2).unwrap(),
        per_call_deadline: Duration::from_millis(10),
        shutdown_deadline: Duration::from_millis(10),
        ordering,
        command_full_policy: ForeignFullPolicy::Reject,
        completion_full_policy: ForeignFullPolicy::Reject,
    }
}

fn abort(code: &str) -> AbortReason {
    AbortReason::new(
        AbortCategory::ForeignException,
        None,
        AbortStage::Runtime,
        AbortRootContext::new(code, "foreign test failure", ConfigMap::empty()),
    )
}

fn text_frame(id: &str, text: &str) -> Frame {
    Frame::new(
        FrameHeader::new(
            FrameId::new(id).unwrap(),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("foreign.test").unwrap(),
                ClockKind::Monotonic,
            ),
            SequenceId::new(0),
            StreamId::new("stream").unwrap(),
            TraceId::new("trace").unwrap(),
            FrameType::Text,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )
        .unwrap(),
        FramePayload::Text(TextData::new(text)),
    )
    .unwrap()
}

#[test]
fn command_mailbox_is_bounded_by_items_and_bytes_without_waiting() {
    let mut item_config = config(ForeignOrdering::Strict);
    item_config.command_capacity = NonZeroUsize::new(1).unwrap();
    let driver = ForeignNodeDriver::new(item_config).unwrap();
    let now = Instant::now();

    assert_eq!(
        driver
            .try_submit(ForeignCommand::new(1, ForeignCommandKind::Prepare), now)
            .unwrap(),
        ForeignSubmitOutcome::Accepted
    );
    assert_eq!(
        driver
            .try_submit(ForeignCommand::new(2, ForeignCommandKind::Finish), now)
            .unwrap(),
        ForeignSubmitOutcome::Full
    );

    let mut byte_config = config(ForeignOrdering::Strict);
    byte_config.command_byte_capacity = NonZeroUsize::new(8).unwrap();
    let byte_driver = ForeignNodeDriver::new(byte_config).unwrap();
    assert_eq!(
        byte_driver
            .try_submit(
                ForeignCommand::new(
                    1,
                    ForeignCommandKind::Process(text_frame("input-bytes", "too large")),
                ),
                now,
            )
            .unwrap(),
        ForeignSubmitOutcome::Full
    );
    assert_eq!(byte_driver.snapshot().in_flight, 0);
}

#[test]
fn default_driver_is_strict_and_single_admission() {
    let config = ForeignDriverConfig::default();
    assert_eq!(config.ordering, ForeignOrdering::Strict);
    assert_eq!(config.max_in_flight.get(), 1);
}

#[test]
fn graceful_stop_seals_an_idle_domain_without_publishing_abort() {
    let driver = ForeignNodeDriver::new(config(ForeignOrdering::Strict)).unwrap();
    assert!(driver.begin_graceful_stop());
    assert!(!driver.begin_graceful_stop());
    assert!(driver.take_abort_reason().is_none());
    assert!(matches!(
        driver.try_receive().unwrap().kind(),
        ForeignCommandKind::Stop
    ));
    assert_eq!(
        driver
            .try_submit(
                ForeignCommand::new(1, ForeignCommandKind::Prepare),
                Instant::now()
            )
            .unwrap(),
        ForeignSubmitOutcome::Closed
    );
}

#[test]
fn strict_driver_releases_only_the_next_contiguous_completion() {
    let driver = ForeignNodeDriver::new(config(ForeignOrdering::Strict)).unwrap();
    let now = Instant::now();
    for sequence in [10, 11] {
        assert_eq!(
            driver
                .try_submit(
                    ForeignCommand::new(sequence, ForeignCommandKind::Prepare),
                    now
                )
                .unwrap(),
            ForeignSubmitOutcome::Accepted
        );
        assert_eq!(driver.try_receive().unwrap().sequence(), sequence);
    }

    assert_eq!(
        driver.try_complete(ForeignCompletion::success(11, [], [], [])),
        ForeignCompletionOutcome::Accepted
    );
    assert!(driver.try_take_completion().is_none());
    assert_eq!(
        driver.try_complete(ForeignCompletion::success(10, [], [], [])),
        ForeignCompletionOutcome::Accepted
    );
    assert_eq!(driver.try_take_completion().unwrap().sequence(), 10);
    assert_eq!(driver.try_take_completion().unwrap().sequence(), 11);
    assert_eq!(driver.snapshot().in_flight, 0);
}

#[test]
fn unordered_driver_releases_completed_work_immediately() {
    let driver = ForeignNodeDriver::new(config(ForeignOrdering::Unordered)).unwrap();
    let now = Instant::now();
    for sequence in [20, 21] {
        driver
            .try_submit(
                ForeignCommand::new(sequence, ForeignCommandKind::Prepare),
                now,
            )
            .unwrap();
        driver.try_receive();
    }

    assert_eq!(
        driver.try_complete(ForeignCompletion::success(21, [], [], [])),
        ForeignCompletionOutcome::Accepted
    );
    assert_eq!(driver.try_take_completion().unwrap().sequence(), 21);
}

#[test]
fn completion_capacity_includes_owned_frame_bytes_and_discards_duplicates() {
    let mut config = config(ForeignOrdering::Unordered);
    config.completion_byte_capacity = NonZeroUsize::new(8).unwrap();
    let driver = ForeignNodeDriver::new(config).unwrap();
    let now = Instant::now();
    driver
        .try_submit(ForeignCommand::new(1, ForeignCommandKind::Prepare), now)
        .unwrap();
    driver.try_receive();

    assert_eq!(
        driver.try_complete(ForeignCompletion::success(
            1,
            [text_frame("output-bytes", "too large")],
            [],
            []
        )),
        ForeignCompletionOutcome::Full
    );
    assert_eq!(driver.snapshot().in_flight, 1);

    assert_eq!(
        driver.try_complete(ForeignCompletion::success(1, [], [], [])),
        ForeignCompletionOutcome::Accepted
    );
    assert_eq!(
        driver.try_complete(ForeignCompletion::success(1, [], [], [])),
        ForeignCompletionOutcome::LateDiscarded
    );
    assert_eq!(driver.try_take_completion().unwrap().sequence(), 1);
}

#[test]
fn deadline_seals_admission_cancels_work_and_discards_late_output() {
    let driver = ForeignNodeDriver::new(config(ForeignOrdering::Strict)).unwrap();
    let now = Instant::now();
    driver
        .try_submit(ForeignCommand::new(1, ForeignCommandKind::Prepare), now)
        .unwrap();
    assert_eq!(driver.try_receive().unwrap().sequence(), 1);

    assert_eq!(driver.expire_deadlines(now + Duration::from_millis(11)), 1);
    assert!(!driver.snapshot().accepting);
    let reason = driver.take_abort_reason().unwrap();
    assert_eq!(reason.root().code(), "MUXIVA-FOREIGN-DEADLINE");
    assert!(driver.take_abort_reason().is_none());
    assert!(matches!(
        driver.try_receive().unwrap().kind(),
        ForeignCommandKind::Cancel
    ));
    assert!(matches!(
        driver.try_receive().unwrap().kind(),
        ForeignCommandKind::Abort(_)
    ));
    assert_eq!(
        driver.try_complete(ForeignCompletion::success(1, [], [], [])),
        ForeignCompletionOutcome::LateDiscarded
    );
    assert!(driver.wait_drained(Instant::now()).is_ok());
}

#[test]
fn explicit_stop_has_one_abort_owner_and_bounded_unfinished_diagnostics() {
    let driver = ForeignNodeDriver::new(config(ForeignOrdering::Strict)).unwrap();
    let now = Instant::now();
    driver
        .try_submit(ForeignCommand::new(1, ForeignCommandKind::Prepare), now)
        .unwrap();
    driver.try_receive();

    assert!(driver.begin_stop(abort("MUXIVA-FOREIGN-STOP")));
    assert!(!driver.begin_stop(abort("MUXIVA-FOREIGN-SECOND")));
    assert_eq!(
        driver
            .try_submit(ForeignCommand::new(2, ForeignCommandKind::Prepare), now)
            .unwrap(),
        ForeignSubmitOutcome::Closed
    );
    assert_eq!(
        driver.take_abort_reason().unwrap().root().code(),
        "MUXIVA-FOREIGN-STOP"
    );
    assert!(driver.take_abort_reason().is_none());

    let diagnostics = driver.wait_drained(Instant::now()).unwrap_err();
    assert_eq!(diagnostics.live_sequences(), [1]);
    assert!(matches!(
        driver.try_receive().unwrap().kind(),
        ForeignCommandKind::Cancel
    ));
    assert!(matches!(
        driver.try_receive().unwrap().kind(),
        ForeignCommandKind::Abort(_)
    ));
    assert!(driver.acknowledge_cancel(1));
    assert!(driver.wait_drained(Instant::now()).is_ok());
}

#[test]
fn failure_completion_becomes_the_single_abort_reason_and_cancels_other_work() {
    let driver = ForeignNodeDriver::new(config(ForeignOrdering::Unordered)).unwrap();
    let now = Instant::now();
    for sequence in [1, 2] {
        driver
            .try_submit(
                ForeignCommand::new(sequence, ForeignCommandKind::Prepare),
                now,
            )
            .unwrap();
        driver.try_receive();
    }

    assert_eq!(
        driver.try_complete(ForeignCompletion::failure(
            1,
            abort("MUXIVA-FOREIGN-FAKE-FAILURE")
        )),
        ForeignCompletionOutcome::Accepted
    );
    assert_eq!(
        driver.take_abort_reason().unwrap().root().code(),
        "MUXIVA-FOREIGN-FAKE-FAILURE"
    );
    assert!(driver.take_abort_reason().is_none());
    assert_eq!(
        driver.try_complete(ForeignCompletion::success(2, [], [], [])),
        ForeignCompletionOutcome::LateDiscarded
    );
    assert!(driver.wait_drained(Instant::now()).is_ok());
}
