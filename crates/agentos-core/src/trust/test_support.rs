//! A screen capture, shared by the image and conversation tests.

use super::{DataSource, ImageFormat, UntrustedImage};

pub(super) fn screenshot() -> UntrustedImage {
    UntrustedImage::new(
        DataSource::Screen {
            target: "Mail".into(),
        },
        ImageFormat::Png,
        vec![0x89, b'P', b'N', b'G'],
        1024,
        768,
    )
}
