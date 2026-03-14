use std::path::Path;

use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit},
};
use zeroize::Zeroizing;

use crate::identity::{DeviceCertificate, DeviceIdentity, Identity, MasterIdentity};

/// Encrypted identity file format:
/// [16 bytes salt][12 bytes nonce][N bytes ciphertext (32-byte key + 16-byte tag)]
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = SALT_LEN + NONCE_LEN;
const VERSION_V2: u8 = 0x02;

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

/// Save a device identity (v2 format): master entropy + device key + certificate.
///
/// File format: `[0x02][16 salt][12 nonce][ciphertext of: 16 entropy + 32 device_key + bincode(cert)]`
pub fn save_device_identity(
    master_entropy: &[u8; 16],
    device: &DeviceIdentity,
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

    // Plaintext: 16 master_entropy + 32 device_key + bincode(certificate)
    let cert_bytes =
        bincode::serialize(device.certificate()).map_err(|_| KeystoreError::InvalidFormat)?;
    let device_key_bytes = device.device_key_bytes();

    let mut plaintext = Vec::with_capacity(16 + 32 + cert_bytes.len());
    plaintext.extend_from_slice(master_entropy);
    plaintext.extend_from_slice(&device_key_bytes);
    plaintext.extend_from_slice(&cert_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|_| KeystoreError::EncryptionFailed)?;

    let mut file_data = Vec::with_capacity(1 + HEADER_LEN + ciphertext.len());
    file_data.push(VERSION_V2);
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

/// Load a device identity from a v2 encrypted file.
///
/// Returns the reconstructed `(MasterIdentity, DeviceIdentity)`.
pub fn load_device_identity(
    passphrase: &[u8],
    path: &Path,
) -> Result<(MasterIdentity, DeviceIdentity), KeystoreError> {
    let file_data = std::fs::read(path)?;

    if file_data.is_empty() || file_data[0] != VERSION_V2 {
        return Err(KeystoreError::InvalidFormat);
    }

    let data = &file_data[1..]; // skip version byte

    if data.len() < HEADER_LEN + 16 + 32 + 16 {
        return Err(KeystoreError::InvalidFormat);
    }

    let salt = &data[..SALT_LEN];
    let nonce_bytes = &data[SALT_LEN..HEADER_LEN];
    let ciphertext = &data[HEADER_LEN..];

    let encryption_key = derive_key(passphrase, salt)?;

    let cipher = ChaCha20Poly1305::new_from_slice(&*encryption_key)
        .map_err(|_| KeystoreError::DecryptionFailed)?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| KeystoreError::DecryptionFailed)?;

    if plaintext.len() < 48 {
        return Err(KeystoreError::InvalidFormat);
    }

    // Parse: 16 entropy + 32 device_key + bincode(cert)
    let mut entropy = Zeroizing::new([0u8; 16]);
    entropy.copy_from_slice(&plaintext[..16]);

    let mut device_key_bytes = [0u8; 32];
    device_key_bytes.copy_from_slice(&plaintext[16..48]);

    let cert: DeviceCertificate =
        bincode::deserialize(&plaintext[48..]).map_err(|_| KeystoreError::InvalidFormat)?;

    let master = MasterIdentity::from_entropy(entropy);
    let device = DeviceIdentity::from_parts(device_key_bytes, cert);

    Ok((master, device))
}

/// Migrate a v1 identity file to v2 format.
///
/// Loads the old `Identity`, creates a new `MasterIdentity` + `DeviceIdentity`,
/// saves as v2, and returns the recovery phrase (display once to the user).
pub fn migrate_v1_to_v2(
    passphrase: &[u8],
    path: &Path,
    device_name: String,
) -> Result<(MasterIdentity, DeviceIdentity, String), KeystoreError> {
    // Load existing v1 identity (validates passphrase)
    let _old_identity = load_identity(passphrase, path)?;

    // Create new master identity + device
    let (master, phrase) = MasterIdentity::generate();
    let device = DeviceIdentity::new(&master, device_name);

    // Save as v2 (overwrites v1 file)
    save_device_identity(master.entropy(), &device, passphrase, path)?;

    Ok((master, device, phrase))
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
    use crate::identity::{DeviceIdentity, MasterIdentity};

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

    #[test]
    fn save_and_load_device_identity_v2() {
        let dir = std::env::temp_dir().join("veil-keystore-v2-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-device.veil");

        let (master, _phrase) = MasterIdentity::generate();
        let device = DeviceIdentity::new(&master, "Test Laptop".into());
        let passphrase = b"correct horse battery staple";

        let master_pid = master.peer_id();
        let device_pid = device.device_peer_id();

        save_device_identity(master.entropy(), &device, passphrase, &path).unwrap();
        let (loaded_master, loaded_device) = load_device_identity(passphrase, &path).unwrap();

        assert_eq!(loaded_master.peer_id(), master_pid);
        assert_eq!(loaded_device.device_peer_id(), device_pid);
        assert_eq!(loaded_device.master_peer_id(), master_pid);
        assert!(loaded_device.certificate().verify());

        // Wrong passphrase fails
        let result = load_device_identity(b"wrong", &path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrate_v1_to_v2_works() {
        let dir = std::env::temp_dir().join("veil-keystore-migrate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-migrate.veil");

        // Save v1 identity
        let identity = Identity::generate();
        let passphrase = b"correct horse battery staple";
        save_identity(&identity, passphrase, &path).unwrap();

        // Migrate
        let (master, device, phrase) =
            migrate_v1_to_v2(passphrase, &path, "Migrated Device".into()).unwrap();

        assert!(!phrase.is_empty());
        assert!(device.certificate().verify());
        assert_eq!(device.master_peer_id(), master.peer_id());

        // Can now load as v2
        let (loaded_master, loaded_device) = load_device_identity(passphrase, &path).unwrap();
        assert_eq!(loaded_master.peer_id(), master.peer_id());
        assert_eq!(loaded_device.device_peer_id(), device.device_peer_id());

        // Old load_identity should fail (format changed)
        let result = load_identity(passphrase, &path);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}
