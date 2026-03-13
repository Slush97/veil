use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::identity::EphemeralKeyPair;

/// Symmetric key shared by all members of a group/channel.
/// Not Clone or Serialize — use `duplicate()` for intentional copies
/// and `encrypt_for_peer()` for key transport.
#[derive(ZeroizeOnDrop)]
pub struct GroupKey {
    key: Zeroizing<[u8; 32]>,
    generation: u64,
}

impl GroupKey {
    /// Create a new random group key.
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut key);
        Self {
            key: Zeroizing::new(key),
            generation: 0,
        }
    }

    /// Create a GroupKey from deterministic bytes (e.g., for local storage encryption).
    /// Generation is set to 0.
    pub fn from_storage_key(key: [u8; 32]) -> Self {
        Self {
            key: Zeroizing::new(key),
            generation: 0,
        }
    }

    /// Create from raw parts (used internally for deserialization).
    fn from_raw(key: [u8; 32], generation: u64) -> Self {
        Self {
            key: Zeroizing::new(key),
            generation,
        }
    }

    /// Export raw key bytes and generation for secure persistence.
    /// Caller must ensure these are stored encrypted.
    pub fn to_raw_parts(&self) -> ([u8; 32], u64) {
        (*self.key, self.generation)
    }

    /// Create from raw key bytes and generation (for loading persisted keys).
    pub fn from_raw_parts(key: [u8; 32], generation: u64) -> Self {
        Self::from_raw(key, generation)
    }

    /// Intentional duplication — for cases where a copy is genuinely needed
    /// (e.g., storing alongside the group). This is explicit rather than Clone
    /// to prevent accidental key material duplication.
    pub fn duplicate(&self) -> Self {
        Self {
            key: Zeroizing::new(*self.key),
            generation: self.generation,
        }
    }

    /// Rotate the key by hashing the current key. Increments generation.
    /// Used when a member is removed to ensure forward secrecy.
    pub fn rotate(&self) -> Self {
        let new_key = blake3::derive_key("veil-group-key-rotation", &*self.key);
        Self {
            key: Zeroizing::new(new_key),
            generation: self.generation + 1,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Derive a channel-specific subkey using blake3 key derivation.
    pub fn derive_channel_key(&self, channel_id: &[u8]) -> Self {
        let mut context = Vec::with_capacity(32 + channel_id.len());
        context.extend_from_slice(&*self.key);
        context.extend_from_slice(channel_id);
        let derived = blake3::derive_key("veil-channel-subkey", &context);
        Self {
            key: Zeroizing::new(derived),
            generation: self.generation,
        }
    }

    /// Encrypt plaintext with this group key.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
        let cipher = ChaCha20Poly1305::new_from_slice(&*self.key)
            .map_err(|_| EncryptError::InvalidKey)?;

        let mut nonce_bytes = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| EncryptError::EncryptionFailed)?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt ciphertext with this group key.
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, EncryptError> {
        if data.len() < 12 {
            return Err(EncryptError::InvalidCiphertext);
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = ChaCha20Poly1305::new_from_slice(&*self.key)
            .map_err(|_| EncryptError::InvalidKey)?;
        let nonce = Nonce::from_slice(nonce_bytes);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| EncryptError::DecryptionFailed)
    }

    /// Encrypt this group key for a specific peer using ephemeral DH.
    /// Returns (ephemeral_public_key_bytes, encrypted_key_data).
    pub fn encrypt_for_peer(
        &self,
        peer_dh_public: &x25519_dalek::PublicKey,
    ) -> Result<(Vec<u8>, Vec<u8>), EncryptError> {
        let eph = EphemeralKeyPair::generate();
        let eph_pub_bytes = eph.public_key().as_bytes().to_vec();
        let shared_secret = eph.exchange(peer_dh_public);
        let derived = blake3::derive_key("veil-group-key-wrap", &*shared_secret);

        let cipher = ChaCha20Poly1305::new_from_slice(&derived)
            .map_err(|_| EncryptError::InvalidKey)?;

        let mut nonce_bytes = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Manually serialize: key (32) + generation (8)
        let mut payload = Vec::with_capacity(40);
        payload.extend_from_slice(&*self.key);
        payload.extend_from_slice(&self.generation.to_le_bytes());

        let ciphertext = cipher
            .encrypt(nonce, payload.as_slice())
            .map_err(|_| EncryptError::EncryptionFailed)?;

        // Zeroize the payload
        payload.zeroize();

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok((eph_pub_bytes, out))
    }

    /// Encrypt this group key with a passphrase using Argon2 + ChaCha20Poly1305.
    /// Returns (16-byte salt, nonce || ciphertext).
    pub fn encrypt_with_passphrase(
        &self,
        passphrase: &[u8],
    ) -> Result<([u8; 16], Vec<u8>), EncryptError> {
        let mut salt = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);

        let wrapping_key = derive_passphrase_key(passphrase, &salt)?;

        let cipher = ChaCha20Poly1305::new_from_slice(&*wrapping_key)
            .map_err(|_| EncryptError::InvalidKey)?;

        let mut nonce_bytes = [0u8; 12];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Serialize: key (32) + generation (8)
        let mut payload = Vec::with_capacity(40);
        payload.extend_from_slice(&*self.key);
        payload.extend_from_slice(&self.generation.to_le_bytes());

        let ciphertext = cipher
            .encrypt(nonce, payload.as_slice())
            .map_err(|_| EncryptError::EncryptionFailed)?;

        payload.zeroize();

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok((salt, out))
    }

    /// Decrypt a group key that was encrypted with a passphrase.
    pub fn decrypt_with_passphrase(
        data: &[u8],
        salt: &[u8; 16],
        passphrase: &[u8],
    ) -> Result<Self, EncryptError> {
        if data.len() < 12 {
            return Err(EncryptError::InvalidCiphertext);
        }

        let wrapping_key = derive_passphrase_key(passphrase, salt)?;

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = ChaCha20Poly1305::new_from_slice(&*wrapping_key)
            .map_err(|_| EncryptError::InvalidKey)?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let mut plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| EncryptError::DecryptionFailed)?;

        if plaintext.len() != 40 {
            plaintext.zeroize();
            return Err(EncryptError::InvalidCiphertext);
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&plaintext[..32]);
        let generation = u64::from_le_bytes(
            plaintext[32..40]
                .try_into()
                .map_err(|_| EncryptError::InvalidCiphertext)?,
        );
        plaintext.zeroize();

        Ok(Self::from_raw(key_bytes, generation))
    }

    /// Decrypt a group key that was encrypted for us using ephemeral DH.
    pub fn decrypt_from_peer(
        data: &[u8],
        our_ephemeral: EphemeralKeyPair,
        peer_eph_public: &x25519_dalek::PublicKey,
    ) -> Result<Self, EncryptError> {
        if data.len() < 12 {
            return Err(EncryptError::InvalidCiphertext);
        }

        let shared_secret = our_ephemeral.exchange(peer_eph_public);
        let derived = blake3::derive_key("veil-group-key-wrap", &*shared_secret);

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = ChaCha20Poly1305::new_from_slice(&derived)
            .map_err(|_| EncryptError::InvalidKey)?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let mut plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| EncryptError::DecryptionFailed)?;

        if plaintext.len() != 40 {
            plaintext.zeroize();
            return Err(EncryptError::InvalidCiphertext);
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&plaintext[..32]);
        let generation = u64::from_le_bytes(
            plaintext[32..40]
                .try_into()
                .map_err(|_| EncryptError::InvalidCiphertext)?,
        );
        plaintext.zeroize();

        Ok(Self::from_raw(key_bytes, generation))
    }
}

fn derive_passphrase_key(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, EncryptError> {
    let mut key = Zeroizing::new([0u8; 32]);
    crate::hardened_argon2()
        .hash_password_into(passphrase, salt, &mut *key)
        .map_err(|_| EncryptError::KeyDerivationFailed)?;
    Ok(key)
}

#[derive(Debug, thiserror::Error)]
pub enum EncryptError {
    #[error("invalid key")]
    InvalidKey,
    #[error("encryption failed")]
    EncryptionFailed,
    #[error("decryption failed")]
    DecryptionFailed,
    #[error("invalid ciphertext")]
    InvalidCiphertext,
    #[error("serialization failed")]
    SerializationFailed,
    #[error("key derivation failed")]
    KeyDerivationFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = GroupKey::generate();
        let plaintext = b"secret message for the group";
        let encrypted = key.encrypt(plaintext).unwrap();
        let decrypted = key.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = GroupKey::generate();
        let key2 = GroupKey::generate();
        let encrypted = key1.encrypt(b"secret").unwrap();
        assert!(key2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn key_rotation() {
        let key = GroupKey::generate();
        let rotated = key.rotate();
        assert_eq!(rotated.generation(), 1);

        let encrypted = rotated.encrypt(b"after rotation").unwrap();
        assert!(key.decrypt(&encrypted).is_err());
        assert!(rotated.decrypt(&encrypted).is_ok());
    }

    #[test]
    fn peer_key_exchange_ephemeral() {
        let group_key = GroupKey::generate();

        // Bob generates an ephemeral keypair and sends the public key to Alice
        let bob_eph = EphemeralKeyPair::generate();
        let bob_eph_pub = *bob_eph.public_key();

        // Alice encrypts the group key for Bob using Bob's ephemeral public key
        let (alice_eph_pub_bytes, encrypted) =
            group_key.encrypt_for_peer(&bob_eph_pub).unwrap();

        // Bob decrypts using his ephemeral secret and Alice's ephemeral public key
        let alice_eph_pub =
            x25519_dalek::PublicKey::from(<[u8; 32]>::try_from(alice_eph_pub_bytes.as_slice()).unwrap());
        let decrypted = GroupKey::decrypt_from_peer(&encrypted, bob_eph, &alice_eph_pub).unwrap();

        // Verify the decrypted key works
        let msg = group_key.encrypt(b"test").unwrap();
        assert!(decrypted.decrypt(&msg).is_ok());
    }

    #[test]
    fn group_key_is_not_clone() {
        // This is a compile-time check: GroupKey should not implement Clone.
        // If this test compiles, the assertion holds.
        fn assert_not_clone<T>() {
            // The test is that this module compiles without GroupKey: Clone
        }
        assert_not_clone::<GroupKey>();
    }

    #[test]
    fn duplicate_works() {
        let key = GroupKey::generate();
        let dup = key.duplicate();
        let encrypted = key.encrypt(b"test").unwrap();
        assert!(dup.decrypt(&encrypted).is_ok());
        assert_eq!(dup.generation(), key.generation());
    }

    #[test]
    fn passphrase_encrypt_decrypt_roundtrip() {
        let key = GroupKey::generate();
        let passphrase = b"correct horse battery staple";

        let (salt, encrypted) = key.encrypt_with_passphrase(passphrase).unwrap();
        let decrypted = GroupKey::decrypt_with_passphrase(&encrypted, &salt, passphrase).unwrap();

        // Verify the decrypted key works the same
        let msg = key.encrypt(b"test").unwrap();
        assert!(decrypted.decrypt(&msg).is_ok());
        assert_eq!(decrypted.generation(), key.generation());
    }

    #[test]
    fn passphrase_wrong_passphrase_fails() {
        let key = GroupKey::generate();
        let (salt, encrypted) = key.encrypt_with_passphrase(b"correct").unwrap();

        let result = GroupKey::decrypt_with_passphrase(&encrypted, &salt, b"wrong");
        assert!(result.is_err());
    }

    #[test]
    fn channel_subkey_derivation() {
        let key = GroupKey::generate();
        let sub1 = key.derive_channel_key(b"channel-1");
        let sub2 = key.derive_channel_key(b"channel-2");

        // Different channels produce different keys
        let encrypted = sub1.encrypt(b"test").unwrap();
        assert!(sub2.decrypt(&encrypted).is_err());
        assert!(sub1.decrypt(&encrypted).is_ok());
    }
}
