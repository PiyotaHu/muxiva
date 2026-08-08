use std::fmt;

use crate::{ErrorCategory, FrameBuffer, MuxivaError, Result};

/// The scalar encoding of one PCM sample.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PcmSampleFormat {
    /// Unsigned 8-bit PCM.
    U8,
    /// Signed little-endian 16-bit PCM.
    I16Le,
    /// Signed little-endian 24-bit PCM.
    I24Le,
    /// Signed little-endian 32-bit PCM.
    I32Le,
    /// Little-endian 32-bit floating-point PCM.
    F32Le,
    /// Little-endian 64-bit floating-point PCM.
    F64Le,
}

impl PcmSampleFormat {
    /// Returns the encoded width of one sample.
    pub const fn bytes_per_sample(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16Le => 2,
            Self::I24Le => 3,
            Self::I32Le | Self::F32Le => 4,
            Self::F64Le => 8,
        }
    }
}

/// Describes how channel samples are arranged in the buffer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioLayout {
    /// Samples for every channel are grouped at each time index.
    Interleaved,
    /// Each channel occupies one contiguous plane, in channel order.
    Planar,
}

/// Validated immutable PCM audio.
#[derive(Clone, Eq, PartialEq)]
pub struct AudioData {
    buffer: FrameBuffer,
    sample_rate_hz: u32,
    channels: u16,
    sample_format: PcmSampleFormat,
    layout: AudioLayout,
    samples_per_channel: u64,
    duration_ns: u64,
}

impl AudioData {
    /// Creates PCM audio after validating scalar limits and exact payload length.
    pub fn new(
        buffer: FrameBuffer,
        sample_rate_hz: u32,
        channels: u16,
        sample_format: PcmSampleFormat,
        layout: AudioLayout,
        samples_per_channel: u64,
    ) -> Result<Self> {
        if !(1..=768_000).contains(&sample_rate_hz) {
            return Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-FRM-AUDIO-RATE",
                "audio sample rate must be between 1 and 768000 Hz",
            ));
        }
        if !(1..=1_024).contains(&channels) {
            return Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-FRM-AUDIO-CHANNELS",
                "audio channel count must be between 1 and 1024",
            ));
        }
        if samples_per_channel == 0 {
            return Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-FRM-AUDIO-SAMPLES",
                "audio must contain at least one sample per channel",
            ));
        }

        let bytes_per_sample =
            u64::try_from(sample_format.bytes_per_sample()).map_err(|_| arithmetic_error())?;
        let expected_bytes = checked_product(samples_per_channel, u64::from(channels))?;
        let expected_bytes = checked_product(expected_bytes, bytes_per_sample)?;
        let duration_ns = duration_ns_for_samples(samples_per_channel, sample_rate_hz)?;
        let expected_len = usize::try_from(expected_bytes).map_err(|_| arithmetic_error())?;

        if buffer.len() != expected_len {
            return Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-FRM-AUDIO-LENGTH",
                "audio payload length does not match its declared layout",
            )
            .with_context("expected_bytes", expected_bytes.to_string())
            .with_context("actual_bytes", buffer.len().to_string()));
        }

        Ok(Self {
            buffer,
            sample_rate_hz,
            channels,
            sample_format,
            layout,
            samples_per_channel,
            duration_ns,
        })
    }

    /// Returns the immutable PCM payload.
    pub fn buffer(&self) -> &FrameBuffer {
        &self.buffer
    }

    /// Returns the sample rate in hertz.
    pub const fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    /// Returns the channel count.
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns the PCM sample encoding.
    pub const fn sample_format(&self) -> PcmSampleFormat {
        self.sample_format
    }

    /// Returns the channel layout.
    pub const fn layout(&self) -> AudioLayout {
        self.layout
    }

    /// Returns the number of samples in each channel.
    pub const fn samples_per_channel(&self) -> u64 {
        self.samples_per_channel
    }

    /// Returns the floor of the audio duration in nanoseconds.
    pub const fn duration_ns(&self) -> u64 {
        self.duration_ns
    }

    /// Returns a channel plane for planar audio.
    ///
    /// For interleaved audio, plane zero is the complete buffer. No other
    /// interleaved plane exists. For planar audio, indices in `0..channels`
    /// return the corresponding contiguous channel plane.
    pub fn plane_bytes(&self, plane: u16) -> Result<&[u8]> {
        match self.layout {
            AudioLayout::Interleaved if plane == 0 => Ok(self.buffer.as_slice()),
            AudioLayout::Interleaved => Err(invalid_plane_error()),
            AudioLayout::Planar if plane >= self.channels => Err(invalid_plane_error()),
            AudioLayout::Planar => {
                let bytes_per_sample = u64::try_from(self.sample_format.bytes_per_sample())
                    .map_err(|_| arithmetic_error())?;
                let plane_bytes = checked_product(self.samples_per_channel, bytes_per_sample)?;
                let start = checked_product(u64::from(plane), plane_bytes)?;
                let end = start
                    .checked_add(plane_bytes)
                    .ok_or_else(arithmetic_error)?;
                let start = usize::try_from(start).map_err(|_| arithmetic_error())?;
                let end = usize::try_from(end).map_err(|_| arithmetic_error())?;
                self.buffer
                    .as_slice()
                    .get(start..end)
                    .ok_or_else(arithmetic_error)
            }
        }
    }
}

impl fmt::Debug for AudioData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioData")
            .field("buffer_len", &self.buffer.len())
            .field("sample_rate_hz", &self.sample_rate_hz)
            .field("channels", &self.channels)
            .field("sample_format", &self.sample_format)
            .field("layout", &self.layout)
            .field("samples_per_channel", &self.samples_per_channel)
            .field("duration_ns", &self.duration_ns)
            .finish()
    }
}

fn checked_product(left: u64, right: u64) -> Result<u64> {
    left.checked_mul(right).ok_or_else(arithmetic_error)
}

pub(crate) fn duration_ns_for_samples(samples: u64, sample_rate_hz: u32) -> Result<u64> {
    let rate = u64::from(sample_rate_hz);
    let whole_seconds = samples / rate;
    let remaining_samples = samples % rate;
    whole_seconds
        .checked_mul(1_000_000_000)
        .and_then(|whole| {
            remaining_samples
                .checked_mul(1_000_000_000)
                .and_then(|fraction| whole.checked_add(fraction / rate))
        })
        .ok_or_else(arithmetic_error)
}

fn arithmetic_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-FRM-ARITHMETIC",
        "frame size arithmetic overflowed",
    )
}

fn invalid_plane_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-FRM-AUDIO-PLANE",
        "audio plane index is not part of the layout",
    )
}
