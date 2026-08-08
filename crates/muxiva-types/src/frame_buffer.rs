use std::{fmt, sync::Arc};

/// Immutable, reference-counted bytes owned by Muxiva.
#[derive(Clone)]
pub struct FrameBuffer(Arc<[u8]>);

impl FrameBuffer {
    /// Moves a vector into immutable shared storage.
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(Arc::from(bytes))
    }

    /// Moves a boxed byte slice into immutable shared storage.
    pub fn from_boxed_slice(bytes: Box<[u8]>) -> Self {
        Self(Arc::from(bytes))
    }

    /// Returns the immutable bytes.
    ///
    /// ```compile_fail
    /// use muxiva_types::FrameBuffer;
    ///
    /// let buffer = FrameBuffer::from_vec(vec![1, 2, 3]);
    /// buffer.as_slice()[0] = 9;
    /// ```
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns the byte length.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the buffer has no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl PartialEq for FrameBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for FrameBuffer {}

impl fmt::Debug for FrameBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameBuffer")
            .field("len", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::FrameBuffer;

    #[test]
    fn allocation_releases_after_last_clone() {
        let buffer = FrameBuffer::from_vec(vec![1, 2, 3]);
        let weak = Arc::downgrade(&buffer.0);
        let clone = buffer.clone();
        drop(buffer);
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn clone_shares_the_same_byte_allocation() {
        let buffer = FrameBuffer::from_vec(vec![1, 2, 3]);
        let clone = buffer.clone();

        assert_eq!(buffer.as_slice().as_ptr(), clone.as_slice().as_ptr());
        assert_eq!(buffer, clone);
    }

    #[test]
    fn empty_buffer_and_debug_are_length_only() {
        let buffer = FrameBuffer::from_boxed_slice(Box::new([]));

        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
        assert_eq!(format!("{buffer:?}"), "FrameBuffer { len: 0 }");
        assert!(!format!("{buffer:?}").contains("strong"));
    }
}
