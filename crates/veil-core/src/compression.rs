//! Lossless compression for the pre-encryption pipeline.
//!
//! All data is prepended with a two-byte header before encryption:
//! - `[0xC0, 0x00]` — uncompressed (raw payload)
//! - `[0xC0, 0x01]` — zstd-compressed payload
//!
//! The magic byte `0xC0` cannot appear as the first byte of any valid
//! bincode-serialized `MessageContent` (bincode v1 serializes enum
//! discriminants as 4-byte LE u32, so byte 0 is always 0x00–0x08).
//! This allows unambiguous detection of legacy pre-compression data.

/// Magic byte marking the start of a compression header.
const MAGIC: u8 = 0xC0;
/// Format: uncompressed payload follows.
const FORMAT_RAW: u8 = 0x00;
/// Format: zstd-compressed payload follows.
const FORMAT_ZSTD: u8 = 0x01;
/// Default zstd compression level (fast, good ratio for chat data).
const ZSTD_LEVEL: i32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum CompressionError {
    #[error("compression failed: {0}")]
    CompressFailed(String),
    #[error("decompression failed: {0}")]
    DecompressFailed(String),
    #[error("decompressed size {size} exceeds limit {limit}")]
    SizeLimitExceeded { size: usize, limit: usize },
}

/// Compress data and prepend a two-byte header (`[MAGIC, format]`).
///
/// Falls back to `FORMAT_RAW` if compression does not reduce size,
/// so the caller never pays a size penalty.
pub fn compress(data: &[u8]) -> Result<Vec<u8>, CompressionError> {
    let compressed = zstd::encode_all(std::io::Cursor::new(data), ZSTD_LEVEL)
        .map_err(|e| CompressionError::CompressFailed(e.to_string()))?;

    if compressed.len() < data.len() {
        let mut out = Vec::with_capacity(2 + compressed.len());
        out.push(MAGIC);
        out.push(FORMAT_ZSTD);
        out.extend_from_slice(&compressed);
        Ok(out)
    } else {
        let mut out = Vec::with_capacity(2 + data.len());
        out.push(MAGIC);
        out.push(FORMAT_RAW);
        out.extend_from_slice(data);
        Ok(out)
    }
}

/// Read the header and decompress if needed.
///
/// `max_size` caps the decompressed output to guard against decompression bombs.
/// Data without the magic prefix is treated as legacy (pre-compression) and
/// returned as-is.
pub fn decompress(data: &[u8], max_size: usize) -> Result<Vec<u8>, CompressionError> {
    if data.len() >= 2 && data[0] == MAGIC {
        match data[1] {
            FORMAT_RAW => Ok(data[2..].to_vec()),
            FORMAT_ZSTD => {
                let decompressed =
                    zstd::decode_all(std::io::Cursor::new(&data[2..]))
                        .map_err(|e| CompressionError::DecompressFailed(e.to_string()))?;
                if decompressed.len() > max_size {
                    return Err(CompressionError::SizeLimitExceeded {
                        size: decompressed.len(),
                        limit: max_size,
                    });
                }
                Ok(decompressed)
            }
            other => Err(CompressionError::DecompressFailed(format!(
                "unknown format byte 0x{other:02x}"
            ))),
        }
    } else {
        // Legacy data without compression header — return as-is.
        Ok(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let data = b"hello world, this is a test message for compression";
        let compressed = compress(data).unwrap();
        assert_eq!(compressed[0], MAGIC);
        let decompressed = decompress(&compressed, 1024).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn incompressible_data_uses_raw() {
        // Random-ish data that won't compress well
        let data: Vec<u8> = (0..64).map(|i| (i * 37 + 13) as u8).collect();
        let compressed = compress(&data).unwrap();
        assert_eq!(compressed[0], MAGIC);
        assert_eq!(compressed[1], FORMAT_RAW);
        let decompressed = decompress(&compressed, 1024).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn legacy_data_passthrough() {
        // Simulate pre-compression bincode data (starts with 0x00, a valid enum discriminant)
        let legacy = vec![0x00, 0x00, 0x00, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o'];
        let result = decompress(&legacy, 1024).unwrap();
        assert_eq!(result, legacy);
    }

    #[test]
    fn size_limit_enforced() {
        // Compress a large repetitive payload, then try to decompress with a tiny limit
        let data = vec![0xAA; 10_000];
        let compressed = compress(&data).unwrap();
        let result = decompress(&compressed, 100);
        assert!(matches!(result, Err(CompressionError::SizeLimitExceeded { .. })));
    }

    #[test]
    fn empty_input() {
        let compressed = compress(b"").unwrap();
        let decompressed = decompress(&compressed, 1024).unwrap();
        assert!(decompressed.is_empty());
    }
}
