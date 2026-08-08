//! NotificationBus subscribers enqueue owned events into a domain; they never call Python.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muxiva_core::{ForeignCommand, ForeignNodeDriver, ForeignSubmitOutcome};
use muxiva_types::EventFrame;

pub(crate) fn try_enqueue_event(
    driver: &ForeignNodeDriver,
    sequence: &Arc<Mutex<u64>>,
    event: EventFrame,
    timeout: Duration,
) -> bool {
    let mut next = sequence.lock().unwrap_or_else(|e| e.into_inner());
    let current = *next;
    let accepted = matches!(
        driver.try_submit(
            ForeignCommand::new(current, muxiva_core::ForeignCommandKind::Event(event)),
            std::time::Instant::now()
        ),
        Ok(ForeignSubmitOutcome::Accepted)
    );
    if accepted {
        *next = next.saturating_add(1);
    }
    drop(next);
    if !accepted {
        return false;
    }
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if driver.try_take_completion().is_some() {
            return true;
        }
        if driver.take_abort_reason().is_some() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    driver.expire_deadlines(Instant::now());
    false
}
