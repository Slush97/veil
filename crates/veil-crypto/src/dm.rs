//! DM (direct message) key derivation for 1-on-1 encrypted channels.
//!
//! Uses static ECDH so both parties derive the same shared key from each
//! other's public keys with no round-trip. Ed25519 keys are converted to
//! X25519 for the DH operation.
//!
//! Security note: Static DH has no forward secrecy for the initial key, but
//! the existing `GroupKeyRing` rotation mechanism provides forward secrecy
//! for subsequent messages.

use ed25519_dalek::SigningKey;

/// Derive a deterministic group ID for a 1-on-1 DM channel.
///
/// Both parties produce the same ID regardless of who initiates. The keys
/// are sorted so `dm_group_id(a, b) == dm_group_id(b, a)`.
pub fn dm_group_id(key_a: &[u8; 32], key_b: &[u8; 32]) -> [u8; 32] {
    let (first, second) = if key_a <= key_b {
        (key_a, key_b)
    } else {
        (key_b, key_a)
    };
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(first.as_slice());
    input.extend_from_slice(second.as_slice());
    blake3::derive_key("veil-dm-group-id-v1", &input)
}

/// Derive a shared symmetric key via Ed25519 → X25519 static Diffie-Hellman.
///
/// Both parties independently call this with their own signing key and the
/// other's verifying key, producing the same shared secret.
pub fn dm_shared_key(my_signing_key: &SigningKey, their_verifying_key: &[u8; 32]) -> [u8; 32] {
    // Convert Ed25519 signing key → X25519 static secret
    // The signing key's scalar is SHA-512(seed)[0..32], clamped
    let my_x25519 = signing_key_to_x25519_secret(my_signing_key);

    // Convert Ed25519 verifying key → X25519 public key (Edwards → Montgomery)
    let their_x25519 = verifying_key_to_x25519_public(their_verifying_key);

    // Perform DH
    let shared_point = my_x25519.diffie_hellman(&their_x25519);

    // KDF the raw DH output into a usable symmetric key
    blake3::derive_key("veil-dm-shared-key-v1", shared_point.as_bytes())
}

/// Generate a verification code from two public keys (like Signal safety numbers).
///
/// Returns a 12-digit numeric code that both parties can compare out-of-band
/// to verify they have the correct keys.
pub fn verification_code(key_a: &[u8; 32], key_b: &[u8; 32]) -> String {
    let (first, second) = if key_a <= key_b {
        (key_a, key_b)
    } else {
        (key_b, key_a)
    };
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(first.as_slice());
    input.extend_from_slice(second.as_slice());
    let hash = blake3::derive_key("veil-verification-code-v1", &input);

    // Take first 6 bytes → 12 hex digits
    let code_bytes = &hash[..6];
    let hex_str = hex::encode(code_bytes);
    // Format as groups of 4 for readability: "ab12 cd34 ef56"
    format!("{} {} {}", &hex_str[0..4], &hex_str[4..8], &hex_str[8..12])
}

/// Convert an Ed25519 signing key to an X25519 static secret.
fn signing_key_to_x25519_secret(signing_key: &SigningKey) -> x25519_dalek::StaticSecret {
    use sha2::Digest;
    let mut hasher = sha2::Sha512::new();
    hasher.update(signing_key.as_bytes());
    let hash = hasher.finalize();

    let mut scalar_bytes = [0u8; 32];
    scalar_bytes.copy_from_slice(&hash[..32]);
    // Clamp (same as what ed25519 does internally)
    scalar_bytes[0] &= 248;
    scalar_bytes[31] &= 127;
    scalar_bytes[31] |= 64;

    x25519_dalek::StaticSecret::from(scalar_bytes)
}

/// Convert an Ed25519 verifying key (compressed Edwards Y) to X25519 public key (Montgomery U).
fn verifying_key_to_x25519_public(verifying_key_bytes: &[u8; 32]) -> x25519_dalek::PublicKey {
    use curve25519_dalek::edwards::CompressedEdwardsY;
    let compressed = CompressedEdwardsY(*verifying_key_bytes);
    let edwards_point = compressed
        .decompress()
        .expect("valid Ed25519 public key must decompress");
    let montgomery = edwards_point.to_montgomery();
    x25519_dalek::PublicKey::from(*montgomery.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn dm_group_id_is_symmetric() {
        let key_a = [1u8; 32];
        let key_b = [2u8; 32];
        assert_eq!(dm_group_id(&key_a, &key_b), dm_group_id(&key_b, &key_a));
    }

    #[test]
    fn dm_group_id_different_for_different_keys() {
        let key_a = [1u8; 32];
        let key_b = [2u8; 32];
        let key_c = [3u8; 32];
        assert_ne!(dm_group_id(&key_a, &key_b), dm_group_id(&key_a, &key_c));
    }

    #[test]
    fn dm_shared_key_is_symmetric() {
        let alice_signing = SigningKey::generate(&mut OsRng);
        let bob_signing = SigningKey::generate(&mut OsRng);

        let alice_pub = alice_signing.verifying_key().to_bytes();
        let bob_pub = bob_signing.verifying_key().to_bytes();

        let alice_shared = dm_shared_key(&alice_signing, &bob_pub);
        let bob_shared = dm_shared_key(&bob_signing, &alice_pub);

        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn dm_shared_key_different_for_different_peers() {
        let alice_signing = SigningKey::generate(&mut OsRng);
        let bob_signing = SigningKey::generate(&mut OsRng);
        let carol_signing = SigningKey::generate(&mut OsRng);

        let bob_pub = bob_signing.verifying_key().to_bytes();
        let carol_pub = carol_signing.verifying_key().to_bytes();

        let shared_ab = dm_shared_key(&alice_signing, &bob_pub);
        let shared_ac = dm_shared_key(&alice_signing, &carol_pub);

        assert_ne!(shared_ab, shared_ac);
    }

    #[test]
    fn verification_code_is_symmetric() {
        let key_a = [10u8; 32];
        let key_b = [20u8; 32];
        assert_eq!(
            verification_code(&key_a, &key_b),
            verification_code(&key_b, &key_a)
        );
    }

    #[test]
    fn verification_code_format() {
        let code = verification_code(&[1u8; 32], &[2u8; 32]);
        // Should be "xxxx xxxx xxxx" format (14 chars total)
        assert_eq!(code.len(), 14);
        assert_eq!(&code[4..5], " ");
        assert_eq!(&code[9..10], " ");
    }
}
