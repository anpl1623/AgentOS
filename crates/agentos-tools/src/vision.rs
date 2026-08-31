//! Preparing captured pixels for a model.
//!
//! Every image AgentOS shows a model passes through [`prepare`] first, for three
//! reasons.
//!
//! **Cost.** A raw display capture is routinely eight megapixels. Providers
//! bill images by area, and a screenshot resized to fit inside 1568 pixels on
//! its long edge costs roughly a tenth of the original while remaining legible
//! to every current vision model. An agent that takes ten screenshots during a
//! run pays for all ten on every subsequent turn, so this is not a rounding
//! error.
//!
//! **Compatibility.** Providers reject images past their own limits. Clamping
//! here means a capture fails at the tool, where the operator can see why, and
//! not four steps later inside an HTTP body.
//!
//! **Safety.** Decoding an image is running a parser over bytes that came from
//! outside the trust boundary — a web page chose them, or a window did. A small
//! file can declare enormous dimensions and ask the decoder to allocate them, so
//! the decoder is given explicit limits and the declared dimensions are checked
//! before any pixels are allocated.

use std::io::Cursor;

use agentos_core::trust::ImageFormat;
use image::{DynamicImage, ImageReader, Limits};
use thiserror::Error;

/// Longest edge, in pixels, an image is resized to fit within.
///
/// Anthropic's guidance and OpenAI's high-detail tiling both stop rewarding
/// resolution at about this size, so past it an agent is paying for pixels no
/// model reads.
pub const DEFAULT_MAX_IMAGE_EDGE: u32 = 1568;

/// Largest encoded image, in bytes, that may be sent to a model.
///
/// Below Anthropic's 5 MB per-image ceiling with room for base64's 4/3 expansion.
pub const DEFAULT_MAX_IMAGE_BYTES: usize = 3 * 1024 * 1024;

/// Ceiling on the pixels a decoder may be asked to allocate, ~64 megapixels at
/// four bytes each. Larger than any real screen and far below a decompression
/// bomb.
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// Quality used when an image has to be re-encoded as JPEG to fit the byte cap.
const JPEG_QUALITY: u8 = 80;

/// An image could not be made presentable to a model.
#[derive(Debug, Error)]
pub enum VisionError {
    /// The bytes are not an image in a format we accept.
    #[error("the capture could not be decoded as an image: {message}")]
    Decode {
        /// Detail from the decoder.
        message: String,
    },

    /// The image declares dimensions no screen has.
    #[error("the capture declares {width}x{height} pixels, which exceeds the decoder's limit")]
    TooLarge {
        /// Declared width.
        width: u32,
        /// Declared height.
        height: u32,
    },

    /// It could not be re-encoded small enough to send.
    #[error("the capture is {actual} bytes after re-encoding, over the {limit}-byte limit")]
    StillTooLarge {
        /// Size after every attempt.
        actual: usize,
        /// The budget.
        limit: usize,
    },

    /// Encoding failed.
    #[error("the capture could not be re-encoded: {message}")]
    Encode {
        /// Detail from the encoder.
        message: String,
    },
}

/// An image that is within a model's limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedImage {
    /// Encoding of `data`.
    pub format: ImageFormat,
    /// The encoded bytes.
    pub data: Vec<u8>,
    /// Width after any resizing.
    pub width: u32,
    /// Height after any resizing.
    pub height: u32,
    /// Whether the image was resized to fit `max_edge`.
    pub resized: bool,
    /// Whether it was re-encoded as JPEG to fit `max_bytes`.
    pub recompressed: bool,
}

/// Decode `bytes`, fit them within `max_edge`, and encode them under `max_bytes`.
///
/// PNG is preserved where it fits, because a screenshot of text survives it
/// exactly and JPEG's ringing around glyphs is the one artefact that actually
/// costs a model accuracy. JPEG is the fallback when PNG will not fit.
///
/// # Errors
///
/// [`VisionError`] if the bytes will not decode, declare implausible
/// dimensions, or cannot be encoded small enough.
pub fn prepare(
    bytes: &[u8],
    max_edge: u32,
    max_bytes: usize,
) -> Result<PreparedImage, VisionError> {
    // Read the header with no allocation limit: reading dimensions allocates
    // nothing, and letting the decoder's own limit fire first would turn a
    // decompression bomb into an indistinguishable "could not decode".
    let (width, height) = reader_for(bytes, Limits::no_limits())?
        .into_dimensions()
        .map_err(|error| VisionError::Decode {
            message: error.to_string(),
        })?;
    if u64::from(width) * u64::from(height) * 4 > MAX_DECODED_BYTES {
        return Err(VisionError::TooLarge { width, height });
    }

    // Decode under a real limit anyway. The check above trusts a header that
    // came from outside; this does not.
    let mut limits = Limits::no_limits();
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    let decoded = reader_for(bytes, limits)?
        .decode()
        .map_err(|error| VisionError::Decode {
            message: error.to_string(),
        })?;

    let longest = decoded.width().max(decoded.height());
    let max_edge = max_edge.max(1);
    let (image, resized) = if longest > max_edge {
        // `resize` preserves the aspect ratio and fits inside the box, so the
        // long edge lands on `max_edge` and the short one below it.
        (
            decoded.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3),
            true,
        )
    } else {
        (decoded, false)
    };

    let png = encode_png(&image)?;
    if png.len() <= max_bytes {
        return Ok(PreparedImage {
            format: ImageFormat::Png,
            width: image.width(),
            height: image.height(),
            data: png,
            resized,
            recompressed: false,
        });
    }

    let jpeg = encode_jpeg(&image)?;
    if jpeg.len() > max_bytes {
        return Err(VisionError::StillTooLarge {
            actual: jpeg.len(),
            limit: max_bytes,
        });
    }
    Ok(PreparedImage {
        format: ImageFormat::Jpeg,
        width: image.width(),
        height: image.height(),
        data: jpeg,
        resized,
        recompressed: true,
    })
}

fn reader_for(bytes: &[u8], limits: Limits) -> Result<ImageReader<Cursor<&[u8]>>, VisionError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| VisionError::Decode {
            message: error.to_string(),
        })?;
    reader.limits(limits);
    Ok(reader)
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, VisionError> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|error| VisionError::Encode {
            message: error.to_string(),
        })?;
    Ok(out.into_inner())
}

fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, VisionError> {
    // JPEG has no alpha channel. Flattening onto white matches what the capture
    // looked like on screen far more often than flattening onto black.
    let rgb = image.to_rgb8();
    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    encoder
        .encode_image(&rgb)
        .map_err(|error| VisionError::Encode {
            message: error.to_string(),
        })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC-32 as PNG specifies it, so the bomb test's chunk is one a decoder
    /// will actually parse rather than reject before the guard is reached.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut body = Vec::from(*kind);
        body.extend_from_slice(data);
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
        out
    }

    /// A PNG carrying nothing but a header that claims the given dimensions.
    fn png_header_declaring(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        // 8-bit RGBA, no interlacing.
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        out.extend_from_slice(&chunk(b"IDAT", &[]));
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut buffer = image::RgbaImage::new(width, height);
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            // Noise, so the encoder cannot compress the test away to nothing.
            *pixel = image::Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255]);
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(buffer)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn a_small_image_passes_through_untouched() {
        let prepared = prepare(
            &png(80, 40),
            DEFAULT_MAX_IMAGE_EDGE,
            DEFAULT_MAX_IMAGE_BYTES,
        )
        .expect("small images prepare");
        assert_eq!(prepared.format, ImageFormat::Png);
        assert_eq!((prepared.width, prepared.height), (80, 40));
        assert!(!prepared.resized);
        assert!(!prepared.recompressed);
    }

    #[test]
    fn an_oversized_capture_is_scaled_to_the_long_edge() {
        let prepared =
            prepare(&png(2000, 1000), 500, DEFAULT_MAX_IMAGE_BYTES).expect("large images prepare");
        assert!(prepared.resized);
        assert_eq!(prepared.width, 500);
        // The aspect ratio survives, so the short edge follows the long one down.
        assert_eq!(prepared.height, 250);
    }

    #[test]
    fn aspect_ratio_is_preserved_for_tall_images() {
        let prepared = prepare(&png(400, 1600), 800, DEFAULT_MAX_IMAGE_BYTES).unwrap();
        assert_eq!((prepared.width, prepared.height), (200, 800));
    }

    #[test]
    fn png_gives_way_to_jpeg_only_when_it_will_not_fit() {
        // Noise defeats PNG's compression, so a budget between the two encodings
        // forces the fallback. The same image inside its PNG size stays PNG.
        let noisy = png(600, 600);
        let budget = 100_000;

        let jpeg = prepare(&noisy, 600, budget).expect("jpeg fallback");
        assert_eq!(jpeg.format, ImageFormat::Jpeg);
        assert!(jpeg.recompressed);
        assert!(jpeg.data.len() <= budget);

        let untouched = prepare(&noisy, 600, DEFAULT_MAX_IMAGE_BYTES).expect("png fits");
        assert_eq!(untouched.format, ImageFormat::Png);
        assert!(!untouched.recompressed);
        assert!(untouched.data.len() > budget);
    }

    #[test]
    fn an_impossible_budget_is_an_error_not_a_silent_giant() {
        let error = prepare(&png(600, 600), 600, 8).expect_err("8 bytes is not an image");
        assert!(matches!(error, VisionError::StillTooLarge { limit: 8, .. }));
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused() {
        let error = prepare(b"<html>not a screenshot</html>", 100, 100_000)
            .expect_err("html is not an image");
        assert!(matches!(error, VisionError::Decode { .. }));
    }

    #[test]
    fn a_declared_size_no_screen_has_is_refused_before_decoding() {
        // A structurally valid PNG declaring 60000x60000: 14 gigabytes once
        // decoded, a few dozen bytes on disk. The check has to happen before
        // the allocation, so the file has to be well-formed enough to believe.
        let header = png_header_declaring(60_000, 60_000);

        let error = prepare(&header, DEFAULT_MAX_IMAGE_EDGE, DEFAULT_MAX_IMAGE_BYTES)
            .expect_err("a decompression bomb is refused");
        assert!(matches!(
            error,
            VisionError::TooLarge {
                width: 60_000,
                height: 60_000
            }
        ));
    }
}
