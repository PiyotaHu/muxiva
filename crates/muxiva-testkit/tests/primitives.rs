use std::{sync::Arc, thread, time::Duration};

use muxiva_core::FlowClock;
use muxiva_testkit::{LeakProbe, ManualClock, TestGate, ThreadProbe};

#[test]
fn gate_clock_and_probes_are_deterministic_and_bounded() {
    let gate = Arc::new(TestGate::new("worker-ready", 1));
    let worker_gate = Arc::clone(&gate);
    let worker = thread::spawn(move || {
        worker_gate.arrive_and_wait(Duration::from_secs(1)).unwrap();
    });
    gate.wait_until_arrived(Duration::from_secs(1)).unwrap();
    assert_eq!(gate.arrived(), 1);
    gate.release();
    worker.join().unwrap();

    let clock = ManualClock::new();
    clock.advance(Duration::from_millis(25));
    assert_eq!(clock.now(), Duration::from_millis(25));

    let leak = LeakProbe::default();
    leak.record_create();
    assert_eq!(leak.snapshot().outstanding(), 1);
    leak.record_destroy();
    assert_eq!(leak.snapshot().outstanding(), 0);

    let threads = ThreadProbe::new(2);
    threads.record("test", "one");
    threads.record("test", "two");
    threads.record("test", "three");
    let snapshot = threads.snapshot();
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].event.as_ref(), "two");
}
