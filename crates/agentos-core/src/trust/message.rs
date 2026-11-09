//! A conversation: [`Content`], [`Role`] and [`Message`].
//!
//! [`Content`] is where the trust boundary is drawn in types. Only its
//! `Control` variant is trusted, and nothing converts a tool result into it.

use serde::{Deserialize, Serialize};

use super::{
    ControlContent, ControlOrigin, DataSource, ImageFormat, Trust, UntrustedContent, UntrustedImage,
};
use crate::tool::ToolCall;

/// One piece of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    /// Trusted, operator-authored instructions.
    Control(ControlContent),
    /// Model-generated prose. Not trusted.
    Model(String),
    /// A tool invocation requested by the model. Not trusted; the runtime
    /// validates and authorises it independently.
    ToolCall(ToolCall),
    /// Data from outside the trust boundary.
    Untrusted(UntrustedContent),
    /// An image from outside the trust boundary.
    ///
    /// Carried as its own variant rather than inside [`UntrustedContent`]
    /// because providers transport images as separate content blocks, and
    /// because a runtime that must drop images for a model without vision needs
    /// to drop exactly those parts and keep the text.
    Image(UntrustedImage),
}

impl Content {
    /// Trusted control-plane content.
    #[must_use]
    pub fn control(origin: ControlOrigin, text: impl Into<String>) -> Self {
        Self::Control(ControlContent::new(origin, text))
    }

    /// Untrusted data-plane content.
    #[must_use]
    pub fn untrusted(source: DataSource, body: impl Into<String>) -> Self {
        Self::Untrusted(UntrustedContent::new(source, body))
    }

    /// Untrusted image content.
    #[must_use]
    pub fn image_from(
        source: DataSource,
        format: ImageFormat,
        data: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Self {
        Self::Image(UntrustedImage::new(source, format, data, width, height))
    }

    /// Which plane this content belongs to.
    ///
    /// Note that [`Content::Model`] and [`Content::ToolCall`] report
    /// [`Trust::Untrusted`]: model output never carries authority.
    #[must_use]
    pub const fn trust(&self) -> Trust {
        match self {
            Self::Control(_) => Trust::Control,
            Self::Model(_) | Self::ToolCall(_) | Self::Untrusted(_) | Self::Image(_) => {
                Trust::Untrusted
            }
        }
    }

    /// Convenience predicate for [`Trust::Control`].
    #[must_use]
    pub const fn is_control(&self) -> bool {
        matches!(self.trust(), Trust::Control)
    }

    /// The data source, if this content came from outside.
    #[must_use]
    pub const fn data_source(&self) -> Option<&DataSource> {
        match self {
            Self::Untrusted(inner) => Some(&inner.source),
            Self::Image(image) => Some(&image.source),
            _ => None,
        }
    }

    /// Untrusted image content, if this is an image.
    #[must_use]
    pub const fn image(&self) -> Option<&UntrustedImage> {
        match self {
            Self::Image(image) => Some(image),
            _ => None,
        }
    }

    /// Render this content as the text a model should see.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Control(inner) => inner.text.clone(),
            Self::Model(text) => text.clone(),
            Self::ToolCall(call) => format!("[tool call {} {}]", call.tool, call.id),
            Self::Untrusted(inner) => inner.render(),
            // Rendering an image as text is what a model without vision sees,
            // and what the audit log records. It describes the image; it never
            // claims to be the image.
            Self::Image(image) => image.describe(),
        }
    }
}

/// Who produced a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System instructions.
    System,
    /// The operator, or tool results being fed back in.
    User,
    /// The model.
    Assistant,
}

/// A single conversation turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Who produced it.
    pub role: Role,
    /// Its parts.
    pub content: Vec<Content>,
}

impl Message {
    /// Build a message.
    #[must_use]
    pub fn new(role: Role, content: Vec<Content>) -> Self {
        Self { role, content }
    }

    /// A system message carrying trusted instructions.
    #[must_use]
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(
            Role::System,
            vec![Content::control(ControlOrigin::SystemInstructions, text)],
        )
    }

    /// A user message carrying the trusted objective.
    #[must_use]
    pub fn objective(text: impl Into<String>) -> Self {
        Self::new(
            Role::User,
            vec![Content::control(ControlOrigin::Objective, text)],
        )
    }

    /// An assistant message carrying model prose.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![Content::Model(text.into())])
    }

    /// Whether any part of this message came from outside the trust boundary.
    #[must_use]
    pub fn carries_untrusted_data(&self) -> bool {
        self.content.iter().any(|c| {
            c.data_source()
                .is_some_and(DataSource::is_externally_influenced)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::test_support::screenshot;

    #[test]
    fn model_output_is_not_control_plane() {
        assert_eq!(Content::Model("hi".into()).trust(), Trust::Untrusted);
        assert!(!Content::Model("hi".into()).is_control());
    }

    #[test]
    fn control_content_is_control_plane() {
        assert!(Content::control(ControlOrigin::Objective, "do the thing").is_control());
    }

    #[test]
    fn images_are_never_control_plane() {
        let content = Content::Image(screenshot());
        assert_eq!(content.trust(), Trust::Untrusted);
        assert!(!content.is_control());
    }

    #[test]
    fn images_carry_their_provenance_for_taint() {
        let message = Message::new(Role::User, vec![Content::Image(screenshot())]);
        assert!(message.carries_untrusted_data());
        assert_eq!(
            message.content[0].data_source(),
            Some(&DataSource::Screen {
                target: "Mail".into()
            })
        );
    }

    #[test]
    fn an_image_renders_as_a_description_not_as_pixels() {
        let rendered = Content::Image(screenshot()).render();
        assert!(rendered.contains("image/png"));
        assert!(rendered.contains("1024x768"));
        assert!(rendered.contains("screen:Mail"));
        // Whatever a model without vision is shown, it is not the bytes.
        assert!(!rendered.contains("PNG"));
    }

    #[test]
    fn message_detects_untrusted_parts() {
        let clean = Message::objective("do the thing");
        assert!(!clean.carries_untrusted_data());

        let dirty = Message::new(
            Role::User,
            vec![Content::untrusted(
                DataSource::Web {
                    url: "https://x".into(),
                },
                "...",
            )],
        );
        assert!(dirty.carries_untrusted_data());
    }
}
