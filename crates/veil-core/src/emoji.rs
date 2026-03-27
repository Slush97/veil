//! Custom emoji support for groups.
//!
//! Custom emoji are small images stored as encrypted blobs and referenced
//! by shortcode (e.g., `:party_parrot:`). Groups maintain an emoji registry
//! that maps shortcodes to blob IDs.

use serde::{Deserialize, Serialize};
use veil_crypto::PeerId;

use crate::message::BlobId;

/// Maximum size for a custom emoji image (256 KB).
pub const MAX_EMOJI_SIZE: usize = 256 * 1024;

/// Maximum shortcode length (without surrounding colons).
pub const MAX_SHORTCODE_LEN: usize = 32;

/// Maximum number of custom emoji per group.
pub const MAX_EMOJI_PER_GROUP: usize = 200;

/// A custom emoji registered in a group.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomEmoji {
    pub shortcode: String,
    pub blob_id: BlobId,
    pub added_by: PeerId,
    pub added_at: i64,
}

/// Validate a shortcode string (without surrounding colons).
/// Valid shortcodes are 1–32 chars of alphanumeric, underscore, or hyphen.
pub fn is_valid_shortcode(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SHORTCODE_LEN
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// Parse `:shortcode:` patterns from text.
/// Returns a list of `(shortcode, start_byte, end_byte)` tuples.
pub fn parse_shortcodes(text: &str) -> Vec<(String, usize, usize)> {
    let mut results = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b':' {
            let start = i;
            i += 1;
            let mut name = String::new();
            while i < bytes.len() && bytes[i] != b':' && bytes[i] != b' ' && bytes[i] != b'\n' {
                name.push(bytes[i] as char);
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b':' && is_valid_shortcode(&name) {
                results.push((name, start, i + 1));
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_shortcodes() {
        assert!(is_valid_shortcode("wave"));
        assert!(is_valid_shortcode("party_parrot"));
        assert!(is_valid_shortcode("thumbs-up"));
        assert!(is_valid_shortcode("a123"));
    }

    #[test]
    fn invalid_shortcodes() {
        assert!(!is_valid_shortcode(""));
        assert!(!is_valid_shortcode("has space"));
        assert!(!is_valid_shortcode("has.dot"));
        assert!(!is_valid_shortcode(&"a".repeat(33)));
    }

    #[test]
    fn parse_single_shortcode() {
        let results = parse_shortcodes(":wave:");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "wave");
        assert_eq!(results[0].1, 0);
        assert_eq!(results[0].2, 6);
    }

    #[test]
    fn parse_shortcodes_in_text() {
        let results = parse_shortcodes("Hey :wave: how are :you:?");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "wave");
        assert_eq!(results[1].0, "you");
    }

    #[test]
    fn parse_no_shortcodes() {
        assert!(parse_shortcodes("no emoji here").is_empty());
    }

    #[test]
    fn parse_rejects_whitespace_in_shortcode() {
        assert!(parse_shortcodes(": space :").is_empty());
    }

    #[test]
    fn parse_rejects_unclosed_colon() {
        assert!(parse_shortcodes(":unclosed").is_empty());
    }
}
