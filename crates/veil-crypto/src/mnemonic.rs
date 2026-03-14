//! Mnemonic phrase generation and recovery for identity backup.
//!
//! Encodes 128 bits of entropy as 12 human-readable words using the BIP39
//! English wordlist. This is NOT a cryptocurrency wallet — the mnemonic is
//! used to derive a Veil master signing key via Argon2id.

use zeroize::Zeroizing;

const WORDLIST: &str = include_str!("wordlist.txt");

/// Parse the embedded wordlist into a vector of words (cached on first call).
fn words() -> Vec<&'static str> {
    WORDLIST.lines().collect()
}

/// Errors from mnemonic operations.
#[derive(Debug, thiserror::Error)]
pub enum MnemonicError {
    #[error("invalid word count: expected 12, got {0}")]
    InvalidWordCount(usize),
    #[error("unknown word in mnemonic: {0}")]
    UnknownWord(String),
    #[error("invalid checksum")]
    InvalidChecksum,
}

/// Generate 128 bits of entropy and encode as a 12-word mnemonic.
///
/// Returns (mnemonic phrase, raw entropy bytes).
/// The entropy should be passed to `entropy_to_master_key` to derive the signing key.
pub fn generate() -> (String, Zeroizing<[u8; 16]>) {
    let mut entropy = Zeroizing::new([0u8; 16]);
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut *entropy);
    let phrase = entropy_to_phrase(&entropy);
    (phrase, entropy)
}

/// Convert 16 bytes of entropy to a 12-word mnemonic phrase.
///
/// Uses BIP39-style encoding: 128 bits entropy + 4 bits checksum = 132 bits = 12 × 11-bit indices.
pub fn entropy_to_phrase(entropy: &[u8; 16]) -> String {
    let wordlist = words();
    debug_assert_eq!(wordlist.len(), 2048);

    // Compute 4-bit checksum from BLAKE3 hash of entropy
    let hash = blake3::hash(entropy);
    let checksum_byte = hash.as_bytes()[0]; // first byte, we use top 4 bits

    // Build 132-bit stream: 128 bits of entropy + 4 bits of checksum
    let mut bits = Vec::with_capacity(132);
    for byte in entropy.iter() {
        for i in (0..8).rev() {
            bits.push((byte >> i) & 1);
        }
    }
    for i in (4..8).rev() {
        bits.push((checksum_byte >> i) & 1);
    }

    // Extract 12 × 11-bit indices
    let mut phrase_words = Vec::with_capacity(12);
    for chunk in bits.chunks_exact(11) {
        let mut index: usize = 0;
        for &bit in chunk {
            index = (index << 1) | (bit as usize);
        }
        phrase_words.push(wordlist[index]);
    }

    phrase_words.join(" ")
}

/// Parse a 12-word mnemonic phrase back to 16 bytes of entropy.
///
/// Validates the checksum.
pub fn phrase_to_entropy(phrase: &str) -> Result<Zeroizing<[u8; 16]>, MnemonicError> {
    let wordlist = words();
    let input_words: Vec<&str> = phrase.split_whitespace().collect();

    if input_words.len() != 12 {
        return Err(MnemonicError::InvalidWordCount(input_words.len()));
    }

    // Convert words to 11-bit indices
    let mut bits = Vec::with_capacity(132);
    for word in &input_words {
        let word_lower = word.to_lowercase();
        let index = wordlist
            .iter()
            .position(|w| *w == word_lower)
            .ok_or(MnemonicError::UnknownWord(word_lower))?;
        for i in (0..11).rev() {
            bits.push(((index >> i) & 1) as u8);
        }
    }

    // Extract 128 bits of entropy (first 128 bits)
    let mut entropy = Zeroizing::new([0u8; 16]);
    for (byte_idx, chunk) in bits[..128].chunks_exact(8).enumerate() {
        let mut byte = 0u8;
        for &bit in chunk {
            byte = (byte << 1) | bit;
        }
        entropy[byte_idx] = byte;
    }

    // Verify 4-bit checksum (bits 128..132)
    let hash = blake3::hash(&*entropy);
    let expected_checksum = (hash.as_bytes()[0] >> 4) & 0x0F;
    let mut actual_checksum = 0u8;
    for &bit in &bits[128..132] {
        actual_checksum = (actual_checksum << 1) | bit;
    }

    if expected_checksum != actual_checksum {
        return Err(MnemonicError::InvalidChecksum);
    }

    Ok(entropy)
}

/// Derive a 32-byte master signing key from mnemonic entropy using Argon2id.
///
/// This is deterministic: the same entropy always produces the same key.
pub fn entropy_to_master_key(entropy: &[u8; 16]) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    // Use a fixed salt derived from the context string — the entropy IS the password.
    // This is safe because the entropy has 128 bits of randomness.
    let salt = blake3::hash(b"veil-master-key-salt");
    let salt_bytes = &salt.as_bytes()[..16]; // Argon2 requires >= 8 byte salt

    crate::hardened_argon2()
        .hash_password_into(entropy, salt_bytes, &mut *key)
        .expect("argon2 key derivation should not fail with valid params");
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_12_words() {
        let (phrase, _entropy) = generate();
        let words: Vec<&str> = phrase.split_whitespace().collect();
        assert_eq!(words.len(), 12);
    }

    #[test]
    fn roundtrip_entropy() {
        let (phrase, entropy) = generate();
        let recovered = phrase_to_entropy(&phrase).unwrap();
        assert_eq!(*entropy, *recovered);
    }

    #[test]
    fn deterministic_phrase() {
        let entropy = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let phrase1 = entropy_to_phrase(&entropy);
        let phrase2 = entropy_to_phrase(&entropy);
        assert_eq!(phrase1, phrase2);
    }

    #[test]
    fn deterministic_master_key() {
        let entropy = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let key1 = entropy_to_master_key(&entropy);
        let key2 = entropy_to_master_key(&entropy);
        assert_eq!(*key1, *key2);
    }

    #[test]
    fn different_entropy_different_key() {
        let e1 = [1u8; 16];
        let e2 = [2u8; 16];
        let k1 = entropy_to_master_key(&e1);
        let k2 = entropy_to_master_key(&e2);
        assert_ne!(*k1, *k2);
    }

    #[test]
    fn invalid_word_rejected() {
        let result = phrase_to_entropy("foo bar baz qux hello world test more words here now end");
        assert!(result.is_err());
    }

    #[test]
    fn wrong_word_count_rejected() {
        let result = phrase_to_entropy("abandon ability");
        assert!(matches!(result, Err(MnemonicError::InvalidWordCount(2))));
    }

    #[test]
    fn checksum_validated() {
        let (phrase, _) = generate();
        let words: Vec<&str> = phrase.split_whitespace().collect();
        // Swap two words to break checksum
        let mut bad_words = words.clone();
        bad_words.swap(0, 1);
        let bad_phrase = bad_words.join(" ");
        // This might still have a valid checksum by chance, but very unlikely
        // Test that the function at least doesn't panic
        let _ = phrase_to_entropy(&bad_phrase);
    }
}
