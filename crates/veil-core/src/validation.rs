//! Post-decryption message validation.
//!
//! Enforces field length limits, content size limits, and structural
//! invariants on incoming messages. This layer sits between decryption
//! and processing to reject malformed or malicious content.
//!
//! Note: sender membership is already verified by the crypto layer
//! (`SealedMessage::verify_and_open_with_keyring` checks signatures
//! against `known_members`), so no separate membership check is needed.

use crate::control::ControlMessage;
use crate::message::{MessageContent, MessageKind};

/// Maximum text message length in characters.
pub const MAX_TEXT_LEN: usize = 16_000;
/// Maximum display name length in characters.
pub const MAX_DISPLAY_NAME_LEN: usize = 100;
/// Maximum bio length in characters.
pub const MAX_BIO_LEN: usize = 500;
/// Maximum status length in characters.
pub const MAX_STATUS_LEN: usize = 128;
/// Maximum filename length in bytes.
pub const MAX_FILENAME_LEN: usize = 255;
/// Maximum number of link previews per message.
pub const MAX_LINK_PREVIEWS: usize = 5;
/// Maximum thumbnail size in bytes (256 KB).
pub const MAX_THUMBNAIL_SIZE: usize = 256 * 1024;
/// Maximum emoji shortcode length in characters.
pub const MAX_EMOJI_LEN: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("text too long: {len} chars (max {max})")]
    TextTooLong { len: usize, max: usize },
    #[error("display name too long: {len} chars (max {max})")]
    DisplayNameTooLong { len: usize, max: usize },
    #[error("bio too long: {len} chars (max {max})")]
    BioTooLong { len: usize, max: usize },
    #[error("status too long: {len} chars (max {max})")]
    StatusTooLong { len: usize, max: usize },
    #[error("filename too long: {len} bytes (max {max})")]
    FilenameTooLong { len: usize, max: usize },
    #[error("too many link previews: {count} (max {max})")]
    TooManyPreviews { count: usize, max: usize },
    #[error("thumbnail too large: {len} bytes (max {max})")]
    ThumbnailTooLarge { len: usize, max: usize },
}

/// Validate the content of a decrypted message.
pub fn validate(content: &MessageContent) -> Result<(), ValidationError> {
    validate_kind(&content.kind)
}

fn validate_kind(kind: &MessageKind) -> Result<(), ValidationError> {
    match kind {
        MessageKind::Text(s) => {
            let len = s.chars().count();
            if len > MAX_TEXT_LEN {
                return Err(ValidationError::TextTooLong {
                    len,
                    max: MAX_TEXT_LEN,
                });
            }
        }
        MessageKind::Image { thumbnail, .. } | MessageKind::Video { thumbnail, .. } => {
            if thumbnail.len() > MAX_THUMBNAIL_SIZE {
                return Err(ValidationError::ThumbnailTooLarge {
                    len: thumbnail.len(),
                    max: MAX_THUMBNAIL_SIZE,
                });
            }
        }
        MessageKind::File { filename, .. } => {
            if filename.len() > MAX_FILENAME_LEN {
                return Err(ValidationError::FilenameTooLong {
                    len: filename.len(),
                    max: MAX_FILENAME_LEN,
                });
            }
        }
        MessageKind::Reply { content, .. } => {
            validate_kind(content)?;
        }
        MessageKind::Edit { new_text, .. } => {
            let len = new_text.chars().count();
            if len > MAX_TEXT_LEN {
                return Err(ValidationError::TextTooLong {
                    len,
                    max: MAX_TEXT_LEN,
                });
            }
        }
        MessageKind::LinkPreview { previews, .. } => {
            if previews.len() > MAX_LINK_PREVIEWS {
                return Err(ValidationError::TooManyPreviews {
                    count: previews.len(),
                    max: MAX_LINK_PREVIEWS,
                });
            }
        }
        MessageKind::Control(ctrl) => {
            validate_control(ctrl)?;
        }
        // Reaction, Delete, Audio — no special validation needed
        _ => {}
    }
    Ok(())
}

fn validate_control(ctrl: &ControlMessage) -> Result<(), ValidationError> {
    if let ControlMessage::ProfileUpdate { fields } = ctrl {
        for field in fields {
            match field {
                crate::control::ProfileField::DisplayName(s) => {
                    let len = s.chars().count();
                    if len > MAX_DISPLAY_NAME_LEN {
                        return Err(ValidationError::DisplayNameTooLong {
                            len,
                            max: MAX_DISPLAY_NAME_LEN,
                        });
                    }
                }
                crate::control::ProfileField::Bio(s) => {
                    let len = s.chars().count();
                    if len > MAX_BIO_LEN {
                        return Err(ValidationError::BioTooLong {
                            len,
                            max: MAX_BIO_LEN,
                        });
                    }
                }
                crate::control::ProfileField::Status(s) => {
                    let len = s.chars().count();
                    if len > MAX_STATUS_LEN {
                        return Err(ValidationError::StatusTooLong {
                            len,
                            max: MAX_STATUS_LEN,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ChannelId, MessageContent, MessageKind};

    fn content(kind: MessageKind) -> MessageContent {
        MessageContent {
            kind,
            timestamp: chrono::Utc::now(),
            channel_id: ChannelId::new(),
        }
    }

    #[test]
    fn text_within_limit() {
        let c = content(MessageKind::Text("hello".into()));
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn text_exceeds_limit() {
        let long = "a".repeat(MAX_TEXT_LEN + 1);
        let c = content(MessageKind::Text(long));
        assert!(matches!(
            validate(&c),
            Err(ValidationError::TextTooLong { .. })
        ));
    }

    #[test]
    fn display_name_too_long() {
        let long_name = "a".repeat(MAX_DISPLAY_NAME_LEN + 1);
        let c = content(MessageKind::Control(ControlMessage::ProfileUpdate {
            fields: vec![crate::control::ProfileField::DisplayName(long_name)],
        }));
        assert!(matches!(
            validate(&c),
            Err(ValidationError::DisplayNameTooLong { .. })
        ));
    }

    #[test]
    fn bio_too_long() {
        let long_bio = "b".repeat(MAX_BIO_LEN + 1);
        let c = content(MessageKind::Control(ControlMessage::ProfileUpdate {
            fields: vec![crate::control::ProfileField::Bio(long_bio)],
        }));
        assert!(matches!(
            validate(&c),
            Err(ValidationError::BioTooLong { .. })
        ));
    }

    #[test]
    fn status_too_long() {
        let long_status = "s".repeat(MAX_STATUS_LEN + 1);
        let c = content(MessageKind::Control(ControlMessage::ProfileUpdate {
            fields: vec![crate::control::ProfileField::Status(long_status)],
        }));
        assert!(matches!(
            validate(&c),
            Err(ValidationError::StatusTooLong { .. })
        ));
    }

    #[test]
    fn too_many_link_previews() {
        let previews = (0..6)
            .map(|i| crate::message::EmbedPreview {
                url: format!("https://example.com/{i}"),
                title: None,
                description: None,
                image_url: None,
                site_name: None,
            })
            .collect();
        let c = content(MessageKind::LinkPreview {
            target_id: crate::message::MessageId::from_content(b"test"),
            previews,
        });
        assert!(matches!(
            validate(&c),
            Err(ValidationError::TooManyPreviews { .. })
        ));
    }

    #[test]
    fn filename_too_long() {
        let c = content(MessageKind::File {
            blob_id: crate::message::BlobId([0; 32]),
            filename: "a".repeat(MAX_FILENAME_LEN + 1),
            size_bytes: 0,
            ciphertext_len: 0,
            inline_data: None,
        });
        assert!(matches!(
            validate(&c),
            Err(ValidationError::FilenameTooLong { .. })
        ));
    }

    #[test]
    fn valid_profile_update_passes() {
        let c = content(MessageKind::Control(ControlMessage::ProfileUpdate {
            fields: vec![
                crate::control::ProfileField::DisplayName("Alice".into()),
                crate::control::ProfileField::Bio("Hi there".into()),
                crate::control::ProfileField::Status("Online".into()),
            ],
        }));
        assert!(validate(&c).is_ok());
    }
}
