use reed_solomon_erasure::galois_8::ReedSolomon;
use veil_crypto::GroupKey;

use crate::StoreError;

/// Maximum decompressed blob size (256 MiB).
const MAX_BLOB_SIZE: usize = 256 * 1024 * 1024;

/// Files smaller than this are sent inline in the message (no sharding).
pub const INLINE_THRESHOLD: usize = 1_048_576; // 1 MiB

/// Number of data shards needed to reconstruct.
pub const DATA_SHARDS: usize = 4;
/// Total shards (data + parity).
pub const TOTAL_SHARDS: usize = 7;
/// Parity shards.
pub const PARITY_SHARDS: usize = TOTAL_SHARDS - DATA_SHARDS;

/// A shard of an erasure-coded, encrypted blob.
/// This is what pinners store — completely opaque.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BlobShard {
    pub blob_id: veil_core::BlobId,
    pub shard_index: u8,
    pub total_shards: u8,
    pub data_shards: u8,
    pub shard_size: u32,
    pub data: Vec<u8>,
}

/// Encrypt, then erasure-code a blob into shards for distributed storage.
/// Returns (shards, ciphertext_len) — the ciphertext length is needed for reconstruction.
pub fn encode_blob(
    plaintext: &[u8],
    group_key: &GroupKey,
) -> Result<(Vec<BlobShard>, usize), StoreError> {
    // Compress, then encrypt — shards never contain plaintext
    let compressed = veil_core::compress(plaintext)
        .map_err(|e| StoreError::Compression(e.to_string()))?;
    let ciphertext = group_key
        .encrypt(&compressed)
        .map_err(|e| StoreError::Crypto(e.to_string()))?;

    let ciphertext_len = ciphertext.len();
    let blob_id = veil_core::BlobId(*blake3::hash(&ciphertext).as_bytes());

    // Pad ciphertext to be evenly divisible by DATA_SHARDS
    let shard_size = ciphertext.len().div_ceil(DATA_SHARDS);
    let mut padded = ciphertext;
    padded.resize(shard_size * DATA_SHARDS, 0);

    // Split into data shards
    let mut shards: Vec<Vec<u8>> = padded.chunks(shard_size).map(|c| c.to_vec()).collect();

    // Add empty parity shards
    for _ in 0..PARITY_SHARDS {
        shards.push(vec![0u8; shard_size]);
    }

    // Compute parity
    let rs = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)
        .map_err(|e| StoreError::ErasureCoding(e.to_string()))?;

    rs.encode(&mut shards)
        .map_err(|e| StoreError::ErasureCoding(e.to_string()))?;

    // Package into BlobShards
    let result = shards
        .into_iter()
        .enumerate()
        .map(|(i, data)| BlobShard {
            blob_id: blob_id.clone(),
            shard_index: i as u8,
            total_shards: TOTAL_SHARDS as u8,
            data_shards: DATA_SHARDS as u8,
            shard_size: shard_size as u32,
            data,
        })
        .collect();

    Ok((result, ciphertext_len))
}

/// Reconstruct a blob from any DATA_SHARDS of the TOTAL_SHARDS.
/// Missing shards should be passed as None.
pub fn decode_blob(
    shards: &[Option<BlobShard>],
    original_ciphertext_len: usize,
    group_key: &GroupKey,
) -> Result<Vec<u8>, StoreError> {
    if shards.len() != TOTAL_SHARDS {
        return Err(StoreError::ErasureCoding(format!(
            "expected {} shards, got {}",
            TOTAL_SHARDS,
            shards.len()
        )));
    }

    let shard_size = shards
        .iter()
        .flatten()
        .next()
        .ok_or_else(|| StoreError::ErasureCoding("no shards provided".into()))?
        .shard_size as usize;

    let mut data: Vec<Option<Vec<u8>>> = shards
        .iter()
        .map(|s| s.as_ref().map(|s| s.data.clone()))
        .collect();

    let rs = ReedSolomon::new(DATA_SHARDS, PARITY_SHARDS)
        .map_err(|e| StoreError::ErasureCoding(e.to_string()))?;

    rs.reconstruct(&mut data)
        .map_err(|e| StoreError::ErasureCoding(e.to_string()))?;

    // Reassemble ciphertext from data shards
    let mut ciphertext = Vec::with_capacity(shard_size * DATA_SHARDS);
    for shard in data.iter().take(DATA_SHARDS) {
        ciphertext.extend_from_slice(
            shard
                .as_ref()
                .ok_or_else(|| StoreError::ErasureCoding("reconstruction failed".into()))?,
        );
    }
    ciphertext.truncate(original_ciphertext_len);

    // Decrypt, then decompress
    let decrypted = group_key
        .decrypt(&ciphertext)
        .map_err(|e| StoreError::Crypto(e.to_string()))?;
    veil_core::decompress(&decrypted, MAX_BLOB_SIZE)
        .map_err(|e| StoreError::Compression(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use veil_crypto::GroupKey;

    #[test]
    fn encode_decode_roundtrip() {
        let key = GroupKey::generate();
        let data = b"hello this is a test image pretend its big";

        let (shards, ct_len) = encode_blob(data, &key).unwrap();
        assert_eq!(shards.len(), TOTAL_SHARDS);

        let mut partial: Vec<Option<BlobShard>> = shards.into_iter().map(Some).collect();

        // Drop shards 1, 3, 5 — still have 4 remaining
        partial[1] = None;
        partial[3] = None;
        partial[5] = None;

        let recovered = decode_blob(&partial, ct_len, &key).unwrap();
        assert_eq!(recovered, data);
    }
}
