use std::path::Path;

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use zeroize::Zeroizing;

use crate::identity::Identity;

/// Encrypted identity file format:
/// [16 bytes salt][12 bytes nonce][N bytes ciphertext (32-byte key + 16-byte tag)]
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = SALT_LEN + NONCE_LEN;

#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid passphrase or corrupted keystore")]
    DecryptionFailed,
    #[error("invalid keystore format")]
    InvalidFormat,
    #[error("key derivation failed")]
    KeyDerivation,
    #[error("encryption failed")]
    EncryptionFailed,
}

/// Save an identity to an encrypted file, protected by a passphrase.
pub fn save_identity(
    identity: &Identity,
    passphrase: &[u8],
    path: &Path,
) -> Result<(), KeystoreError> {
    let mut salt = [0u8; SALT_LEN];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);

    let encryption_key = derive_key(passphrase, &salt)?;

    let cipher = ChaCha20Poly1305::new_from_slice(&*encryption_key)
        .map_err(|_| KeystoreError::EncryptionFailed)?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let identity_bytes = identity.to_bytes();
    let ciphertext = cipher
        .encrypt(nonce, identity_bytes.as_slice())
        .map_err(|_| KeystoreError::EncryptionFailed)?;

    let mut file_data = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    file_data.extend_from_slice(&salt);
    file_data.extend_from_slice(&nonce_bytes);
    file_data.extend_from_slice(&ciphertext);

    std::fs::write(path, &file_data)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Load an identity from an encrypted file using a passphrase.
pub fn load_identity(passphrase: &[u8], path: &Path) -> Result<Identity, KeystoreError> {
    let file_data = std::fs::read(path)?;

    if file_data.len() < HEADER_LEN + 32 + 16 {
        return Err(KeystoreError::InvalidFormat);
    }

    let salt = &file_data[..SALT_LEN];
    let nonce_bytes = &file_data[SALT_LEN..HEADER_LEN];
    let ciphertext = &file_data[HEADER_LEN..];

    let encryption_key = derive_key(passphrase, salt)?;

    let cipher = ChaCha20Poly1305::new_from_slice(&*encryption_key)
        .map_err(|_| KeystoreError::DecryptionFailed)?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| KeystoreError::DecryptionFailed)?;

    if plaintext.len() != 32 {
        return Err(KeystoreError::InvalidFormat);
    }

    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&plaintext);
    Ok(Identity::from_bytes(&key_bytes))
}

fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, KeystoreError> {
    let mut key = Zeroizing::new([0u8; 32]);
    crate::hardened_argon2()
        .hash_password_into(passphrase, salt, &mut *key)
        .map_err(|_| KeystoreError::KeyDerivation)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_identity() {
        let dir = std::env::temp_dir().join("veil-keystore-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-identity.veil");

        let identity = Identity::generate();
        let peer_id = identity.peer_id();
        let passphrase = b"correct horse battery staple";

        save_identity(&identity, passphrase, &path).unwrap();
        let loaded = load_identity(passphrase, &path).unwrap();

        assert_eq!(loaded.peer_id().verifying_key, peer_id.verifying_key);

        // Wrong passphrase fails
        let result = load_identity(b"wrong", &path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
