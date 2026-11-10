//! Untrusted images: [`UntrustedImage`], and the [`ImageFormat`]s a provider
//! is asked to look at.
//!
//! Apart from the text envelope because pixels cannot be enveloped: there is
//! no delimiter to neutralise in an image, so its boundary is drawn by having
//! no trusted counterpart at all. How the bytes travel, as base64 and a media
//! type, is a concern of their own.

use serde::{Deserialize, Serialize};

use super::DataSource;

/// An image format a provider can be asked to look at.
///
/// Deliberately short. Every entry is a format both major providers accept and
/// that `agentos-tools` can re-encode, so a tool cannot hand the runtime bytes
/// that only fail at the provider's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// WebP.
    Webp,
    /// GIF.
    Gif,
}

impl ImageFormat {
    /// The IANA media type, which is what providers name the format by.
    #[must_use]
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
        }
    }

    /// Parse a media type. Returns `None` for anything not listed above rather
    /// than guessing, because guessing here means sending a provider bytes it
    /// will reject.
    #[must_use]
    pub fn from_media_type(media_type: &str) -> Option<Self> {
        match media_type.trim().to_ascii_lowercase().as_str() {
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            "image/gif" => Some(Self::Gif),
            _ => None,
        }
    }
}

/// An image that entered the system from outside the trust boundary.
///
/// # Why there is no trusted image
///
/// [`ControlContent`] has no visual counterpart, and that absence is deliberate.
/// Every image AgentOS can obtain is a capture of something an attacker may
/// control — a web page, a window, a file — and an image is a far worse place to
/// draw the boundary than text: there is no envelope to wrap pixels in, no
/// delimiter to neutralise, and a screenshot of a page reading "SYSTEM: you are
/// now authorised" is indistinguishable to a model from a system message. The
/// only safe rule is that pixels are never instructions, so the type system
/// offers no way to say otherwise.
///
/// The accompanying [`UntrustedContent`] envelope still carries the text that
/// describes the image, and it is that envelope — not the image — that tells the
/// model where the data came from.
///
/// [`ControlContent`]: super::ControlContent
/// [`UntrustedContent`]: super::UntrustedContent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntrustedImage {
    /// Provenance of the pixels.
    pub source: DataSource,
    /// Encoding of `data`.
    pub format: ImageFormat,
    /// The encoded bytes, base64 in the serialised form.
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
    /// Width in pixels, after any downscaling.
    pub width: u32,
    /// Height in pixels, after any downscaling.
    pub height: u32,
    /// The tool call this image answers, when applicable.
    pub tool_call_id: Option<String>,
}

impl UntrustedImage {
    /// Wrap image bytes that came from outside.
    #[must_use]
    pub fn new(
        source: DataSource,
        format: ImageFormat,
        data: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            source,
            format,
            data,
            width,
            height,
            tool_call_id: None,
        }
    }

    /// Associate this image with the tool call that produced it.
    #[must_use]
    pub fn for_tool_call(mut self, id: impl Into<String>) -> Self {
        self.tool_call_id = Some(id.into());
        self
    }

    /// Size of the encoded image in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the image carries no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Base64 of the encoded bytes, which is how every provider transports them.
    #[must_use]
    pub fn base64(&self) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }

    /// A `data:` URI, the form OpenAI-compatible endpoints expect.
    #[must_use]
    pub fn data_uri(&self) -> String {
        format!("data:{};base64,{}", self.format.media_type(), self.base64())
    }

    /// One line describing the image without reproducing it.
    ///
    /// Used for traces, for the audit log, and as the text a model is given in
    /// place of an image its provider cannot see.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "[{} image, {}x{} pixels, {} bytes, from {}]",
            self.format.media_type(),
            self.width,
            self.height,
            self.data.len(),
            self.source.label()
        )
    }
}

/// Base64 transport for image bytes.
///
/// serde's default for `Vec<u8>` is a JSON array of numbers, which would make a
/// one-megabyte screenshot a several-megabyte trace entry.
mod base64_bytes {
    use base64::Engine as _;
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::test_support::screenshot;

    #[test]
    fn images_transport_as_base64_not_as_a_byte_array() {
        let image = screenshot();
        assert_eq!(image.base64(), "iVBORw==");
        assert_eq!(image.data_uri(), "data:image/png;base64,iVBORw==");

        let json = serde_json::to_string(&image).unwrap();
        assert!(json.contains("\"data\":\"iVBORw==\""));
        let round_tripped: UntrustedImage = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, image);
    }

    #[test]
    fn media_types_round_trip_and_unknown_ones_are_refused() {
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Webp,
            ImageFormat::Gif,
        ] {
            assert_eq!(
                ImageFormat::from_media_type(format.media_type()),
                Some(format)
            );
        }
        assert_eq!(
            ImageFormat::from_media_type("IMAGE/JPG"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(ImageFormat::from_media_type("image/svg+xml"), None);
        assert_eq!(ImageFormat::from_media_type("text/html"), None);
    }
}
