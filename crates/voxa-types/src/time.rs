/// A signed nanosecond value interpreted within an explicit clock domain.
///
/// ```compile_fail
/// use voxa_types::Timestamp;
/// let earlier = Timestamp::from_nanos(1);
/// let later = Timestamp::from_nanos(2);
/// let _ = earlier < later;
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Timestamp(i64);

impl Timestamp {
    /// Creates a timestamp from signed nanoseconds.
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    /// Returns the timestamp as signed nanoseconds.
    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    /// Returns a timestamp offset by `nanos`, or `None` on overflow.
    pub const fn checked_add(self, nanos: i64) -> Option<Self> {
        match self.0.checked_add(nanos) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// An ordered sequence counter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SequenceId(u64);

impl SequenceId {
    /// Creates a sequence identifier from its counter value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the sequence counter value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next sequence identifier, or `None` on overflow.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SequenceId, Timestamp};

    #[test]
    fn timestamp_and_sequence_checked_arithmetic() {
        let timestamp = Timestamp::from_nanos(20);
        assert_eq!(timestamp.as_nanos(), 20);
        assert_eq!(timestamp.checked_add(22).unwrap().as_nanos(), 42);
        assert!(Timestamp::from_nanos(i64::MAX).checked_add(1).is_none());

        let sequence = SequenceId::new(7);
        assert_eq!(sequence.get(), 7);
        assert_eq!(sequence.checked_next().unwrap().get(), 8);
        assert!(SequenceId::new(u64::MAX).checked_next().is_none());
    }
}
