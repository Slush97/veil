pub mod compression;
pub mod control;
pub mod dedup;
pub mod emoji;
pub mod group;
pub mod invite;
pub mod link;
pub mod media;
pub mod message;
pub mod validation;

pub use compression::{CompressionError, compress, decompress};
pub use control::{ControlMessage, MetadataField, ProfileField};
pub use dedup::{DeduplicateError, MessageDeduplicator};
pub use emoji::{CustomEmoji, is_valid_shortcode, parse_shortcodes};
pub use group::{
    Category, Channel, ChannelKind, Group, GroupError, Member, PermissionOverride, Permissions,
    Role, role_level,
};
pub use invite::{InviteError, InviteKeyMaterial, InvitePayload};
pub use link::{extract_urls, parse_embed_metadata};
pub use media::{
    AudioMeta, ImageMeta, MediaType, detect as detect_media, extract_audio_meta, extract_image_meta,
};
pub use message::{
    BlobId, CategoryId, ChannelId, EmbedPreview, GroupId, MessageContent, MessageId, MessageKind,
    SealedMessage, SealedMessageError, routing_tag_for_group,
};
pub use validation::{ValidationError, validate};
