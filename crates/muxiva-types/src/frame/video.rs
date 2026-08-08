use std::ptr;

use crate::{ErrorCategory, FrameBuffer, MuxivaError, Result};

use super::{checked_size_product, checked_size_sum};

/// The pixel encoding of validated video data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PixelFormat {
    /// Packed red, green, blue, and alpha bytes for each pixel.
    Rgba8,
    /// Planar 8-bit luma with half-resolution chroma in both dimensions.
    Yuv420p,
}

/// Describes one immutable plane inside a video buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoPlane {
    offset: usize,
    stride: usize,
    row_bytes: usize,
    rows: u32,
}

impl VideoPlane {
    /// Returns the plane's byte offset in the video buffer.
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the stored byte distance between consecutive rows.
    pub const fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the number of meaningful pixel bytes in each row.
    pub const fn row_bytes(&self) -> usize {
        self.row_bytes
    }

    /// Returns the number of stored rows in the plane.
    pub const fn rows(&self) -> u32 {
        self.rows
    }
}

/// The validated plane arrangement for a video buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoLayout {
    /// One packed RGBA8 plane.
    Rgba8 { plane: VideoPlane },
    /// Tightly sequenced Y, U, and V planes.
    Yuv420p {
        y: VideoPlane,
        u: VideoPlane,
        v: VideoPlane,
    },
}

/// Validated immutable pixel video.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoData {
    buffer: FrameBuffer,
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    layout: VideoLayout,
}

impl VideoData {
    /// Creates packed RGBA8 video after validating its exact layout and length.
    pub fn rgba8(buffer: FrameBuffer, width: u32, height: u32, stride: usize) -> Result<Self> {
        validate_nonzero_dimensions(width, height)?;

        let width = usize::try_from(width).map_err(|_| arithmetic_error())?;
        let row_bytes = checked_size_product(width, 4)?;
        validate_stride(stride, row_bytes)?;

        let plane_bytes = checked_size_product(
            stride,
            usize::try_from(height).map_err(|_| arithmetic_error())?,
        )?;
        validate_payload_length(&buffer, plane_bytes)?;

        Ok(Self {
            buffer,
            width: u32::try_from(width).map_err(|_| arithmetic_error())?,
            height,
            pixel_format: PixelFormat::Rgba8,
            layout: VideoLayout::Rgba8 {
                plane: VideoPlane {
                    offset: 0,
                    stride,
                    row_bytes,
                    rows: height,
                },
            },
        })
    }

    /// Creates planar YUV420P video after validating its exact layout and length.
    pub fn yuv420p(
        buffer: FrameBuffer,
        width: u32,
        height: u32,
        y_stride: usize,
        u_stride: usize,
        v_stride: usize,
    ) -> Result<Self> {
        validate_yuv420p_dimensions(width, height)?;

        let y_row_bytes = usize::try_from(width).map_err(|_| arithmetic_error())?;
        let chroma_row_bytes = usize::try_from(width / 2).map_err(|_| arithmetic_error())?;
        validate_stride(y_stride, y_row_bytes)?;
        validate_stride(u_stride, chroma_row_bytes)?;
        validate_stride(v_stride, chroma_row_bytes)?;

        let y_rows = height;
        let chroma_rows = height / 2;
        let y_bytes = checked_size_product(
            y_stride,
            usize::try_from(y_rows).map_err(|_| arithmetic_error())?,
        )?;
        let u_bytes = checked_size_product(
            u_stride,
            usize::try_from(chroma_rows).map_err(|_| arithmetic_error())?,
        )?;
        let v_bytes = checked_size_product(
            v_stride,
            usize::try_from(chroma_rows).map_err(|_| arithmetic_error())?,
        )?;
        let v_offset = checked_size_sum(y_bytes, u_bytes)?;
        let total_bytes = checked_size_sum(v_offset, v_bytes)?;
        validate_payload_length(&buffer, total_bytes)?;

        Ok(Self {
            buffer,
            width,
            height,
            pixel_format: PixelFormat::Yuv420p,
            layout: VideoLayout::Yuv420p {
                y: VideoPlane {
                    offset: 0,
                    stride: y_stride,
                    row_bytes: y_row_bytes,
                    rows: y_rows,
                },
                u: VideoPlane {
                    offset: y_bytes,
                    stride: u_stride,
                    row_bytes: chroma_row_bytes,
                    rows: chroma_rows,
                },
                v: VideoPlane {
                    offset: v_offset,
                    stride: v_stride,
                    row_bytes: chroma_row_bytes,
                    rows: chroma_rows,
                },
            },
        })
    }

    /// Returns the immutable pixel payload.
    pub fn buffer(&self) -> &FrameBuffer {
        &self.buffer
    }

    /// Returns the frame width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the frame height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the pixel encoding.
    pub const fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Returns the validated plane arrangement.
    pub fn layout(&self) -> &VideoLayout {
        &self.layout
    }

    /// Returns the plane's full immutable stride-including byte range.
    ///
    /// The descriptor must be one of the exact instances borrowed from this
    /// value's [`Self::layout`]. A descriptor from another value is rejected,
    /// even when all of its scalar fields match.
    pub fn plane_bytes(&self, plane: &VideoPlane) -> Result<&[u8]> {
        let belongs_to_layout = match &self.layout {
            VideoLayout::Rgba8 { plane: own } => ptr::eq(plane, own),
            VideoLayout::Yuv420p { y, u, v } => {
                ptr::eq(plane, y) || ptr::eq(plane, u) || ptr::eq(plane, v)
            }
        };
        if !belongs_to_layout {
            return Err(invalid_plane_error());
        }

        let plane_bytes = checked_size_product(
            plane.stride,
            usize::try_from(plane.rows).map_err(|_| arithmetic_error())?,
        )?;
        let end = checked_size_sum(plane.offset, plane_bytes)?;
        self.buffer
            .as_slice()
            .get(plane.offset..end)
            .ok_or_else(arithmetic_error)
    }
}

fn validate_nonzero_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(dimensions_error());
    }
    Ok(())
}

fn validate_yuv420p_dimensions(width: u32, height: u32) -> Result<()> {
    validate_nonzero_dimensions(width, height)?;
    if width % 2 != 0 || height % 2 != 0 {
        return Err(dimensions_error());
    }
    Ok(())
}

fn validate_stride(stride: usize, row_bytes: usize) -> Result<()> {
    if stride < row_bytes {
        return Err(MuxivaError::new(
            ErrorCategory::Validation,
            "MUXIVA-FRM-VIDEO-STRIDE",
            "video stride is smaller than its plane row width",
        ));
    }
    Ok(())
}

fn validate_payload_length(buffer: &FrameBuffer, expected_bytes: usize) -> Result<()> {
    if buffer.len() != expected_bytes {
        return Err(MuxivaError::new(
            ErrorCategory::Validation,
            "MUXIVA-FRM-VIDEO-LENGTH",
            "video payload length does not match its declared layout",
        )
        .with_context("expected_bytes", expected_bytes.to_string())
        .with_context("actual_bytes", buffer.len().to_string()));
    }
    Ok(())
}

fn arithmetic_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-FRM-ARITHMETIC",
        "frame size arithmetic overflowed",
    )
}

fn dimensions_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-FRM-VIDEO-DIMENSIONS",
        "video dimensions are invalid for the pixel format",
    )
}

fn invalid_plane_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-FRM-VIDEO-PLANE",
        "video plane descriptor is not part of the layout",
    )
}
